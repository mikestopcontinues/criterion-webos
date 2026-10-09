// SPDX-License-Identifier: GPL-3.0-or-later
//! Original bounded reader for the current ARM32 little-endian Linux auxiliary vector.

use std::sync::OnceLock;

pub struct Cache {
    vector: OnceLock<Result<Auxv, Failure>>,
}
impl Cache {
    pub const fn new() -> Self {
        Self {
            vector: OnceLock::new(),
        }
    }
    pub fn getauxval(
        &self,
        kind: u32,
        errno: &mut i32,
        load: impl FnOnce() -> Result<Auxv, Failure>,
    ) -> u32 {
        match self
            .vector
            .get_or_init(load)
            .as_ref()
            .ok()
            .and_then(|auxv| auxv.lookup(kind))
        {
            Some(value) => value,
            None => {
                *errno = 2;
                0
            }
        }
    }
}
impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError {
    Interrupted,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Unavailable,
    Malformed,
    Limit,
}

const MAX_ENTRIES: usize = 64;
const MAX_READS: usize = 1024;

#[derive(Clone, Copy)]
struct Entry {
    kind: u32,
    value: u32,
}
pub struct Auxv {
    entries: [Entry; MAX_ENTRIES],
    len: usize,
}
impl Auxv {
    pub fn read(
        mut read: impl FnMut(&mut [u8]) -> Result<usize, ReadError>,
    ) -> Result<Self, Failure> {
        let mut auxv = Self {
            entries: [Entry { kind: 0, value: 0 }; MAX_ENTRIES],
            len: 0,
        };
        let mut attempts = 0;
        for _ in 0..MAX_ENTRIES {
            let mut pair = [0; 8];
            let mut used = 0;
            while used < pair.len() {
                let count = next(&mut read, &mut pair[used..], &mut attempts)?;
                if count == 0 {
                    return Err(Failure::Malformed);
                }
                used += count;
            }
            let kind = u32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]);
            let value = u32::from_le_bytes([pair[4], pair[5], pair[6], pair[7]]);
            if kind == 0 {
                let mut trailing = [0];
                if value != 0 || next(&mut read, &mut trailing, &mut attempts)? != 0 {
                    return Err(Failure::Malformed);
                }
                return Ok(auxv);
            }
            if auxv.lookup(kind).is_some() {
                return Err(Failure::Malformed);
            }
            auxv.entries[auxv.len] = Entry { kind, value };
            auxv.len += 1;
        }
        Err(Failure::Limit)
    }
    pub fn lookup(&self, kind: u32) -> Option<u32> {
        self.entries[..self.len]
            .iter()
            .find(|entry| entry.kind == kind)
            .map(|entry| entry.value)
    }
}

fn next(
    read: &mut impl FnMut(&mut [u8]) -> Result<usize, ReadError>,
    out: &mut [u8],
    attempts: &mut usize,
) -> Result<usize, Failure> {
    while *attempts < MAX_READS {
        *attempts += 1;
        match read(out) {
            Ok(count) if count <= out.len() => return Ok(count),
            Ok(_) => return Err(Failure::Malformed),
            Err(ReadError::Interrupted) => continue,
            Err(ReadError::Unavailable) => return Err(Failure::Unavailable),
        }
    }
    Err(Failure::Limit)
}

#[cfg(all(
    feature = "webos",
    target_os = "linux",
    target_arch = "arm",
    target_pointer_width = "32",
    target_endian = "little"
))]
mod native {
    use super::{Auxv, Cache, Failure, ReadError};
    use std::{
        arch::asm,
        ffi::{c_int, c_ulong},
    };

    static CACHE: Cache = Cache::new();
    const _: () = {
        assert!(size_of::<c_ulong>() == 4);
        assert!(size_of::<c_int>() == 4);
    };
    unsafe extern "C" {
        fn __errno_location() -> *mut c_int;
    }

    /// Current Linux ARM EABI: r7 number, r0-r2 arguments, signed r0 result.
    /// Preserve frame/base registers and balanced 8-byte stack alignment. No libc call,
    /// Rust allocation, logging, symbol delegation or cache access can re-enter initialization.
    unsafe fn syscall<const NUMBER: u32>(a: u32, b: u32, c: u32) -> i32 {
        let result: u32;
        unsafe {
            asm!(
                "push {{r7, r12}}",
                "mov r7, #{number}",
                "svc #0",
                "pop {{r7, r12}}",
                number = const NUMBER,
                inlateout("r0") a => result,
                inlateout("r1") b => _,
                inlateout("r2") c => _,
            );
        }
        result as i32
    }

    struct Descriptor(u32);
    impl Descriptor {
        fn open() -> Result<Self, Failure> {
            // Fixed proc data only; O_RDONLY | O_CLOEXEC | O_NONBLOCK.
            for _ in 0..8 {
                let result =
                    unsafe { syscall::<5>(c"/proc/self/auxv".as_ptr() as u32, 0x80000 | 0x800, 0) };
                if result >= 0 {
                    return Ok(Self(result as u32));
                }
                if result != -4 {
                    return Err(Failure::Unavailable);
                }
            }
            Err(Failure::Limit)
        }
        fn read(&self, out: &mut [u8]) -> Result<usize, ReadError> {
            let result = unsafe { syscall::<3>(self.0, out.as_mut_ptr() as u32, out.len() as u32) };
            match result {
                -4 => Err(ReadError::Interrupted),
                result if result < 0 => Err(ReadError::Unavailable),
                result => Ok(result as usize),
            }
        }
    }
    impl Drop for Descriptor {
        fn drop(&mut self) {
            // Linux retires the descriptor even when close returns EINTR; never retry it.
            unsafe {
                syscall::<6>(self.0, 0, 0);
            }
        }
    }
    fn load() -> Result<Auxv, Failure> {
        let descriptor = Descriptor::open()?;
        Auxv::read(|out| descriptor.read(out))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn getauxval(kind: c_ulong) -> c_ulong {
        // Errno is thread-local. Keep only a raw pointer across OnceLock waiting; no mutable
        // Rust reference to libc TLS aliases an internal futex/libc errno write.
        let pointer = unsafe { __errno_location() };
        let mut errno = unsafe { *pointer };
        let value = CACHE.getauxval(kind, &mut errno, load);
        unsafe {
            *pointer = errno;
        }
        value
    }
}
