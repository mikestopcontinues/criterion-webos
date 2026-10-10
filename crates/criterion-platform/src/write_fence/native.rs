// SPDX-License-Identifier: GPL-3.0-or-later
//! One fixed MAIN registration on the fence worker's private GLib context.
#[path = "native/ffi.rs"]
mod ffi;
use super::{Db8Transport, FenceError, store::MAX_REPLY};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
    sync::atomic::{AtomicBool, Ordering},
    thread::ThreadId,
    time::{Duration, Instant},
};

const NAME: &CStr = c"com.mikestopcontinues.criterion.unofficial";
const APP: &CStr = c"com.mikestopcontinues.criterion.unofficial";
const GET: &CStr = c"luna://com.palm.db/get";
const PUT: &CStr = c"luna://com.palm.db/put";
const GET_REQUEST: &CStr = c"{\"ids\":[\"crit.write.v1\"]}";
const CALL_LIMIT: Duration = Duration::from_secs(2);
static CLAIMED: AtomicBool = AtomicBool::new(false);

struct Slot {
    token: ffi::Token,
    active: bool,
    delivered: bool,
    result: Option<Result<Vec<u8>, FenceError>>,
}
impl Slot {
    fn new() -> Box<Self> {
        Box::new(Self {
            token: 0,
            active: false,
            delivered: false,
            result: None,
        })
    }
}

pub(super) struct NativeTransport {
    handle: *mut ffi::Handle,
    context: *mut ffi::Context,
    slot: Option<Box<Slot>>,
    owner: ThreadId,
    closed: bool,
    claimed: bool,
}
// Only the private worker factory can construct this type. It creates, uses and drops it on
// that same worker. No handle/context/callback pointer is exposed or accessed concurrently.
unsafe impl Send for NativeTransport {}

impl NativeTransport {
    pub(super) fn open() -> Result<Self, FenceError> {
        if CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(FenceError::Unavailable);
        }
        let mut bus = Self {
            handle: ptr::null_mut(),
            context: unsafe { ffi::g_main_context_new() },
            slot: Some(Slot::new()),
            owner: std::thread::current().id(),
            closed: false,
            claimed: true,
        };
        if bus.context.is_null() {
            return Err(FenceError::Unavailable);
        }
        let mut error = ffi::Error::new();
        if !unsafe {
            ffi::LSRegisterApplicationService(
                NAME.as_ptr(),
                APP.as_ptr(),
                &mut bus.handle,
                &mut error,
            )
        } || bus.handle.is_null()
        {
            return Err(FenceError::Unavailable);
        }
        if unsafe { bounded(ffi::LSHandleGetName(bus.handle), 128) } != Some(NAME.to_bytes()) {
            return Err(FenceError::Unconfirmed);
        }
        let mut error = ffi::Error::new();
        if !unsafe { ffi::LSGmainContextAttach(bus.handle, bus.context, &mut error) } {
            return Err(FenceError::Unavailable);
        }
        Ok(bus)
    }

    fn call(
        &mut self,
        uri: &'static CStr,
        payload: &CStr,
        deadline: Instant,
    ) -> Result<Vec<u8>, FenceError> {
        if self.closed || self.owner != std::thread::current().id() {
            return Err(FenceError::Unavailable);
        }
        let expires = deadline.min(Instant::now() + CALL_LIMIT);
        if Instant::now() >= expires {
            self.dispose();
            return Err(FenceError::Unconfirmed);
        }
        let Some(slot) = self.slot.as_mut() else {
            return Err(FenceError::Unavailable);
        };
        slot.token = 0;
        slot.active = true;
        slot.delivered = false;
        slot.result = None;
        let data = ptr::from_mut(slot.as_mut()).cast::<c_void>();
        let mut token = 0;
        let mut error = ffi::Error::new();
        let accepted = unsafe {
            ffi::LSCallOneReply(
                self.handle,
                uri.as_ptr(),
                payload.as_ptr(),
                receive,
                data,
                &mut token,
                &mut error,
            )
        };
        // There was no iteration during issuance. Token assignment precedes every dispatch.
        if let Some(slot) = self.slot.as_mut() {
            slot.token = token;
        }
        if !accepted || token == 0 {
            self.dispose();
            return Err(FenceError::Unconfirmed);
        }
        loop {
            if Instant::now() >= expires {
                self.dispose();
                return Err(FenceError::Unconfirmed);
            }
            let dispatched = unsafe { ffi::g_main_context_iteration(self.context, 0) != 0 };
            if let Some(result) = self.slot.as_mut().and_then(|slot| slot.result.take()) {
                // A late acknowledgment never admits an operation; cancellation is not remote cancellation.
                if Instant::now() >= expires || result.is_err() {
                    self.dispose();
                    return Err(FenceError::Unconfirmed);
                }
                return result;
            }
            if !dispatched {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    fn dispose(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let token = self
            .slot
            .as_mut()
            .map(|slot| {
                slot.active = false;
                if slot.delivered { 0 } else { slot.token }
            })
            .unwrap_or(0);
        if !self.handle.is_null() {
            if token != 0 {
                let mut error = ffi::Error::new();
                unsafe {
                    ffi::LSCallCancel(self.handle, token, &mut error);
                }
            }
            let mut error = ffi::Error::new();
            if !unsafe { ffi::LSUnregister(self.handle, &mut error) } {
                // Callback ownership is unconfirmed: retain the one stable slot and context
                // until process exit. The fixed registration claim stays held; reopening
                // cannot turn this bounded quarantine into repeated allocation.
                if let Some(slot) = self.slot.take() {
                    std::mem::forget(slot);
                }
                self.context = ptr::null_mut();
                self.handle = ptr::null_mut();
                return;
            }
            self.handle = ptr::null_mut();
        }
        if !self.context.is_null() {
            unsafe {
                ffi::g_main_context_unref(self.context);
            }
            self.context = ptr::null_mut();
        }
        if self.claimed {
            CLAIMED.store(false, Ordering::Release);
            self.claimed = false;
        }
    }
}
impl Drop for NativeTransport {
    fn drop(&mut self) {
        self.dispose();
    }
}
impl Db8Transport for NativeTransport {
    fn get(&mut self, deadline: Instant) -> Result<Vec<u8>, FenceError> {
        self.call(GET, GET_REQUEST, deadline)
    }
    fn put(
        &mut self,
        expected_rev: u64,
        possibly_issued: bool,
        deadline: Instant,
    ) -> Result<Vec<u8>, FenceError> {
        let payload = format!(
            r#"{{"objects":[{{"_id":"crit.write.v1","_kind":"com.mikestopcontinues.criterion.unofficial.issuedwrite:1","_rev":{expected_rev},"version":1,"possiblyIssued":{possibly_issued}}}]}}"#
        );
        let payload = CString::new(payload).map_err(|_| FenceError::Invalid)?;
        self.call(PUT, &payload, deadline)
    }
}

/// LS2 owns a readable NUL-terminated field throughout this callback. Scan within the
/// fixed limit before borrowing a slice; no pointer or external diagnostic is retained.
unsafe fn bounded<'a>(pointer: *const c_char, limit: usize) -> Option<&'a [u8]> {
    if pointer.is_null() {
        return None;
    }
    for length in 0..=limit {
        if unsafe { *pointer.add(length) } == 0 {
            return Some(unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) });
        }
    }
    None
}
unsafe extern "C" fn receive(
    _handle: *mut ffi::Handle,
    message: *mut ffi::Message,
    data: *mut c_void,
) -> bool {
    if data.is_null() || message.is_null() {
        return true;
    }
    let slot = unsafe { &mut *data.cast::<Slot>() };
    if !slot.active || slot.delivered {
        return true;
    }
    slot.delivered = true;
    slot.result = Some(super::reply::admit(
        slot.token.into(),
        unsafe { ffi::LSMessageGetResponseToken(message) }.into(),
        unsafe { bounded(ffi::LSMessageGetSenderServiceName(message), 128) },
        unsafe { ffi::LSMessageIsHubErrorMessage(message) },
        unsafe { bounded(ffi::LSMessageGetPayload(message), MAX_REPLY) },
    ));
    true
}
