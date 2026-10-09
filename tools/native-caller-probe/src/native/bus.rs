use super::ffi;
use criterion_native_caller_probe::{
    Failure, KeyAdmission, Snapshot, admit_bridge, admit_keymanager,
};
use std::{
    ffi::{CStr, c_char, c_void},
    marker::PhantomData,
    ptr,
    rc::Rc,
};

const NAME: &CStr = c"com.mikestopcontinues.criterion.probe.native.caller";
const APP: &CStr = c"com.mikestopcontinues.criterion.probe.native";

#[derive(Clone, Copy)]
pub enum Method {
    Attach,
    Ping,
    Close,
    Keymanager,
}
impl Method {
    fn index(self) -> usize {
        match self {
            Self::Attach => 0,
            Self::Ping => 1,
            Self::Close => 2,
            Self::Keymanager => 3,
        }
    }
    fn request(self) -> (&'static CStr, &'static CStr) {
        match self {
            Self::Attach => (
                c"luna://com.mikestopcontinues.criterion.probe.player.bridge/attach",
                c"{\"version\":\"0.1.0\",\"subscribe\":true}",
            ),
            Self::Ping => (
                c"luna://com.mikestopcontinues.criterion.probe.player.bridge/ping",
                c"{\"version\":\"0.1.0\"}",
            ),
            Self::Close => (
                c"luna://com.mikestopcontinues.criterion.probe.player.bridge/close",
                c"{\"version\":\"0.1.0\"}",
            ),
            Self::Keymanager => (
                c"luna://com.webos.service.keymanager3/exportKey",
                c"{\"name\":\"criterion.native-caller.absent.v1\"}",
            ),
        }
    }
}
#[derive(Clone, Copy)]
pub enum Reply {
    Bridge(Snapshot),
    Keymanager(KeyAdmission),
}
struct Slot {
    method: Method,
    token: ffi::Token,
    active: bool,
    delivered: bool,
    result: Option<Result<Reply, Failure>>,
}
impl Slot {
    fn new(method: Method) -> Box<Self> {
        Box::new(Self {
            method,
            token: 0,
            active: false,
            delivered: false,
            result: None,
        })
    }
}

/// Single main-thread LS2 owner. No GLib iteration may occur after disposal starts.
pub struct Bus {
    handle: *mut ffi::Handle,
    context: *mut ffi::Context,
    slots: Option<[Box<Slot>; 4]>,
    disposed: bool,
    _main_thread: PhantomData<Rc<()>>,
}

impl Bus {
    pub fn open() -> Result<Self, &'static str> {
        let mut bus = Self {
            handle: ptr::null_mut(),
            context: unsafe { ffi::g_main_context_new() },
            slots: Some([
                Slot::new(Method::Attach),
                Slot::new(Method::Ping),
                Slot::new(Method::Close),
                Slot::new(Method::Keymanager),
            ]),
            disposed: false,
            _main_thread: PhantomData,
        };
        if bus.context.is_null() {
            return Err("context-unavailable");
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
            return Err("registration-rejected");
        }
        let name = unsafe { bounded(ffi::LSHandleGetName(bus.handle), 128) };
        if name != Some(NAME.to_bytes()) {
            return Err("registration-name-mismatch");
        }
        let mut error = ffi::Error::new();
        if !unsafe { ffi::LSGmainContextAttach(bus.handle, bus.context, &mut error) } {
            return Err("context-attach-rejected");
        }
        Ok(bus)
    }

    pub fn call(&mut self, method: Method) -> bool {
        if self.disposed {
            return false;
        }
        let (uri, payload) = method.request();
        let data = {
            let Some(slots) = self.slots.as_mut() else {
                return false;
            };
            let slot = &mut slots[method.index()];
            // Each fixed method is issued at most once in this process.
            if slot.active || slot.token != 0 {
                return false;
            }
            slot.active = true;
            ptr::from_mut(slot.as_mut()).cast::<c_void>()
        };
        let mut token = 0;
        let mut error = ffi::Error::new();
        let accepted = unsafe {
            let call = if matches!(method, Method::Attach) {
                ffi::LSCall
            } else {
                ffi::LSCallOneReply
            };
            call(
                self.handle,
                uri.as_ptr(),
                payload.as_ptr(),
                receive,
                data,
                &mut token,
                &mut error,
            )
        };
        // LS2 dispatches on our private context; no iteration happened during issuance.
        let slot = &mut self.slots.as_mut().expect("live fixed slots")[method.index()];
        slot.token = token;
        if !accepted || token == 0 {
            slot.active = false;
            return false;
        }
        true
    }

    pub fn dispatch_one(&mut self) -> bool {
        !self.disposed && unsafe { ffi::g_main_context_iteration(self.context, 0) != 0 }
    }
    pub fn take(&mut self, method: Method) -> Option<Result<Reply, Failure>> {
        self.slots.as_mut()?.get_mut(method.index())?.result.take()
    }

    pub fn dispose(&mut self) -> bool {
        if self.disposed {
            return self.handle.is_null();
        }
        self.disposed = true;
        let tokens = if let Some(slots) = self.slots.as_mut() {
            for slot in slots.iter_mut() {
                slot.active = false;
            }
            slots.each_ref().map(|slot| {
                if matches!(slot.method, Method::Attach) || !slot.delivered {
                    slot.token
                } else {
                    0
                }
            })
        } else {
            [0; 4]
        };
        for token in tokens {
            if token != 0 {
                let mut error = ffi::Error::new();
                // Cancellation acceptance does not establish remote broker retirement.
                unsafe {
                    ffi::LSCallCancel(self.handle, token, &mut error);
                }
            }
        }
        if !self.handle.is_null() {
            let mut error = ffi::Error::new();
            if !unsafe { ffi::LSUnregister(self.handle, &mut error) } {
                // LS2 may still retain callback pointers and context references. Quarantine the
                // fixed four boxes and native context until process exit instead of freeing them.
                if let Some(slots) = self.slots.take() {
                    std::mem::forget(slots);
                }
                self.context = ptr::null_mut();
                return false;
            }
            self.handle = ptr::null_mut();
        }
        if !self.context.is_null() {
            unsafe {
                ffi::g_main_context_unref(self.context);
            }
            self.context = ptr::null_mut();
        }
        true
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        if !self.disposed && !self.dispose() {
            eprintln!("native-caller: bus-cleanup-unconfirmed");
        }
    }
}

/// The native library owns a readable NUL-terminated field for this callback's duration.
/// Bound scanning before creating a Rust slice; never retain a field or diagnostic.
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
    // Stable boxes outlive every issued call and remain alive if unregister fails.
    let slot = unsafe { &mut *data.cast::<Slot>() };
    if !slot.active || slot.delivered {
        return true;
    }
    let response = if unsafe { ffi::LSMessageIsHubErrorMessage(message) } {
        Err(Failure::HubRejected)
    } else {
        let source = unsafe { bounded(ffi::LSMessageGetSenderServiceName(message), 128) }
            .and_then(|value| std::str::from_utf8(value).ok());
        let payload = unsafe { bounded(ffi::LSMessageGetPayload(message), 4096) };
        let token = unsafe { ffi::LSMessageGetResponseToken(message) } as u64;
        match payload {
            None => Err(Failure::InvalidResponse),
            Some(payload) => match slot.method {
                Method::Keymanager => admit_keymanager(source, token, slot.token as u64, payload)
                    .map(Reply::Keymanager),
                _ => admit_bridge(source, token, slot.token as u64, payload).map(Reply::Bridge),
            },
        }
    };
    // Preserve the initial attach snapshot. Heartbeats cannot replace its baseline or reopen it.
    slot.result = Some(response);
    slot.delivered = true;
    true
}
