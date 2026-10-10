//! Narrow stock-SDK declarations. `main_bus/abi.c` checks these against shipped headers.
use std::ffi::{c_char, c_int, c_ulong, c_void};

#[repr(C)]
pub struct Handle {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Message {
    _private: [u8; 0],
}
#[repr(C)]
pub struct Context {
    _private: [u8; 0],
}
pub type Token = c_ulong;
pub type Callback = unsafe extern "C" fn(*mut Handle, *mut Message, *mut c_void) -> bool;

#[repr(C)]
pub struct Error {
    code: c_int,
    message: *mut c_char,
    file: *const c_char,
    line: c_int,
    function: *const c_char,
    padding: *mut c_void,
    magic: c_ulong,
}
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(size_of::<Token>() == 4);
    assert!(size_of::<Error>() == 28);
    assert!(align_of::<Error>() == 4);
    assert!(std::mem::offset_of!(Error, code) == 0);
    assert!(std::mem::offset_of!(Error, message) == 4);
    assert!(std::mem::offset_of!(Error, file) == 8);
    assert!(std::mem::offset_of!(Error, line) == 12);
    assert!(std::mem::offset_of!(Error, function) == 16);
    assert!(std::mem::offset_of!(Error, padding) == 20);
    assert!(std::mem::offset_of!(Error, magic) == 24);
};

impl Error {
    pub fn new() -> Self {
        // LSErrorInit initializes every field before any other LS2 operation.
        let mut error: Self = unsafe { std::mem::zeroed() };
        unsafe {
            LSErrorInit(&mut error);
        }
        error
    }
}
impl Drop for Error {
    fn drop(&mut self) {
        unsafe {
            LSErrorFree(self);
        }
    }
}

#[cfg(not(test))]
#[link(name = "luna-service2")]
unsafe extern "C" {
    fn LSErrorInit(error: *mut Error) -> bool;
    fn LSErrorFree(error: *mut Error);
    pub fn LSRegisterApplicationService(
        name: *const c_char,
        app_id: *const c_char,
        handle: *mut *mut Handle,
        error: *mut Error,
    ) -> bool;
    pub fn LSHandleGetName(handle: *mut Handle) -> *const c_char;
    pub fn LSGmainContextAttach(
        handle: *mut Handle,
        context: *mut Context,
        error: *mut Error,
    ) -> bool;
    pub fn LSCallOneReply(
        handle: *mut Handle,
        uri: *const c_char,
        payload: *const c_char,
        callback: Callback,
        data: *mut c_void,
        token: *mut Token,
        error: *mut Error,
    ) -> bool;
    pub fn LSCallCancel(handle: *mut Handle, token: Token, error: *mut Error) -> bool;
    pub fn LSUnregister(handle: *mut Handle, error: *mut Error) -> bool;
    pub fn LSMessageGetSenderServiceName(message: *mut Message) -> *const c_char;
    pub fn LSMessageGetPayload(message: *mut Message) -> *const c_char;
    pub fn LSMessageGetResponseToken(message: *mut Message) -> Token;
    pub fn LSMessageIsHubErrorMessage(message: *mut Message) -> bool;
}
#[cfg(not(test))]
#[link(name = "glib-2.0")]
unsafe extern "C" {
    pub fn g_main_context_new() -> *mut Context;
    // gboolean is a C int, unlike the LS2 C bool above.
    pub fn g_main_context_iteration(context: *mut Context, may_block: c_int) -> c_int;
    pub fn g_main_context_unref(context: *mut Context);
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
pub(super) use fixture::{
    attach as LSGmainContextAttach, call as LSCallOneReply, cancel as LSCallCancel,
    context_new as g_main_context_new, context_unref as g_main_context_unref,
    error_free as LSErrorFree, error_init as LSErrorInit, handle_name as LSHandleGetName,
    hub_error as LSMessageIsHubErrorMessage, iterate as g_main_context_iteration,
    payload as LSMessageGetPayload, register as LSRegisterApplicationService,
    sender as LSMessageGetSenderServiceName, token as LSMessageGetResponseToken,
    unregister as LSUnregister,
};
