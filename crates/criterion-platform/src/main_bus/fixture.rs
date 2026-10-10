// SPDX-License-Identifier: GPL-3.0-or-later
//! Controlled host LS2/GLib calls. Never contacts a bus or loads native libraries.
use super::{Callback, Context, Error, Handle, Message, Token};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    sync::{Mutex, MutexGuard, mpsc},
    thread::ThreadId,
    time::Duration,
};

#[derive(Clone, Copy, Default)]
pub(crate) enum Mode {
    #[default]
    Normal,
    WrongSender,
    WrongToken,
}
#[derive(Default)]
struct State {
    registrations: usize,
    contexts: usize,
    unregisters: usize,
    unrefs: usize,
    threads: Vec<ThreadId>,
    calls: Vec<String>,
    revision: u64,
    issued: bool,
    next_token: Token,
    pending: Option<(Callback, usize, Token, CString)>,
    mode: Mode,
    unregister_failure: bool,
    gate: Option<(mpsc::SyncSender<()>, mpsc::Receiver<()>)>,
}
static SERIAL: Mutex<()> = Mutex::new(());
static STATE: Mutex<Option<State>> = Mutex::new(None);

pub(crate) struct Fixture(MutexGuard<'static, ()>);
impl Fixture {
    pub(crate) fn new() -> Self {
        let serial = SERIAL.lock().unwrap();
        *STATE.lock().unwrap() = Some(State {
            revision: 7,
            next_token: 40,
            ..State::default()
        });
        Self(serial)
    }
    pub(crate) fn mode(&self, mode: Mode) {
        state(|state| state.mode = mode);
    }
    pub(crate) fn fail_unregister(&self) {
        state(|state| state.unregister_failure = true);
    }
    pub(crate) fn hold_dispatch(&self) -> (mpsc::Receiver<()>, mpsc::SyncSender<()>) {
        let (entered, entry) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        state(|state| state.gate = Some((entered, released)));
        (entry, release)
    }
    pub(crate) fn assert_closed(&self, calls: &[&str]) {
        state(|state| {
            assert_eq!(state.registrations, 1);
            assert_eq!(state.contexts, 1);
            assert_eq!(state.unregisters, 1);
            assert_eq!(state.unrefs, 1);
            assert_eq!(state.calls, calls);
            assert!(state.pending.is_none());
            assert!(!state.threads.is_empty());
            assert!(
                state
                    .threads
                    .iter()
                    .all(|thread| *thread == state.threads[0])
            );
            assert_ne!(state.threads[0], std::thread::current().id());
        });
    }
    pub(crate) fn assert_quarantined(&self) {
        state(|state| {
            assert_eq!(state.registrations, 1);
            assert_eq!(state.contexts, 1);
            assert_eq!(state.unregisters, 1);
            assert_eq!(state.unrefs, 0);
        });
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // The guard serializes native-body fixtures. Quarantine is tested in a subprocess.
        let _ = &self.0;
        *STATE.lock().unwrap() = None;
    }
}
fn state<T>(f: impl FnOnce(&mut State) -> T) -> T {
    f(STATE
        .lock()
        .unwrap()
        .as_mut()
        .expect("installed FFI fixture"))
}
fn event() {
    state(|state| state.threads.push(std::thread::current().id()));
}
pub(crate) unsafe fn error_init(_: *mut Error) -> bool {
    true
}
pub(crate) unsafe fn error_free(_: *mut Error) {}
pub(crate) unsafe fn context_new() -> *mut Context {
    event();
    state(|state| state.contexts += 1);
    std::ptr::NonNull::<Context>::dangling().as_ptr()
}
pub(crate) unsafe fn register(
    name: *const c_char,
    app: *const c_char,
    handle: *mut *mut Handle,
    _: *mut Error,
) -> bool {
    event();
    assert_eq!(
        unsafe { CStr::from_ptr(name) }.to_bytes(),
        b"com.mikestopcontinues.criterion.unofficial"
    );
    assert_eq!(
        unsafe { CStr::from_ptr(app) }.to_bytes(),
        b"com.mikestopcontinues.criterion.unofficial"
    );
    state(|state| state.registrations += 1);
    unsafe { *handle = std::ptr::NonNull::<Handle>::dangling().as_ptr() };
    true
}
pub(crate) unsafe fn handle_name(_: *mut Handle) -> *const c_char {
    event();
    c"com.mikestopcontinues.criterion.unofficial".as_ptr()
}
pub(crate) unsafe fn attach(_: *mut Handle, _: *mut Context, _: *mut Error) -> bool {
    event();
    true
}
pub(crate) unsafe fn call(
    _: *mut Handle,
    uri: *const c_char,
    payload: *const c_char,
    callback: Callback,
    data: *mut c_void,
    token: *mut Token,
    _: *mut Error,
) -> bool {
    event();
    let uri = unsafe { CStr::from_ptr(uri) }.to_str().unwrap();
    let payload: serde_json::Value =
        serde_json::from_slice(unsafe { CStr::from_ptr(payload) }.to_bytes()).unwrap();
    state(|state| {
        assert!(state.pending.is_none());
        state.calls.push(uri.to_owned());
        let response = match uri {
            "luna://com.palm.db/get" => {
                assert_eq!(payload, serde_json::json!({"ids":["crit.write.v1"]}));
                format!(
                    r#"{{"returnValue":true,"results":[{{"_id":"crit.write.v1","_kind":"com.mikestopcontinues.criterion.unofficial.issuedwrite:1","_rev":{},"version":1,"possiblyIssued":{}}}]}}"#,
                    state.revision, state.issued
                )
            }
            "luna://com.palm.db/put" => {
                assert_eq!(
                    payload,
                    serde_json::json!({"objects":[{"_id":"crit.write.v1","_kind":"com.mikestopcontinues.criterion.unofficial.issuedwrite:1","_rev":state.revision,"version":1,"possiblyIssued":!state.issued}]})
                );
                state.revision += 1;
                state.issued = !state.issued;
                format!(
                    r#"{{"returnValue":true,"results":[{{"id":"crit.write.v1","rev":{}}}]}}"#,
                    state.revision
                )
            }
            _ => panic!("non-DB8 route"),
        };
        state.next_token += 1;
        unsafe { *token = state.next_token };
        state.pending = Some((
            callback,
            data as usize,
            state.next_token,
            CString::new(response).unwrap(),
        ));
    });
    true
}
struct Reply {
    token: Token,
    sender: &'static CStr,
    payload: CString,
}
pub(crate) unsafe fn iterate(_: *mut Context, may_block: c_int) -> c_int {
    event();
    assert_eq!(may_block, 0);
    let (pending, mode, gate) =
        state(|state| (state.pending.take(), state.mode, state.gate.take()));
    if let Some((entered, release)) = gate {
        entered.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(1)).unwrap();
    }
    let Some((callback, data, token, payload)) = pending else {
        return 0;
    };
    let mut reply = Reply {
        token: if matches!(mode, Mode::WrongToken) {
            token - 1
        } else {
            token
        },
        sender: if matches!(mode, Mode::WrongSender) {
            c"other.db"
        } else {
            c"com.palm.db"
        },
        payload,
    };
    unsafe {
        callback(
            std::ptr::NonNull::<Handle>::dangling().as_ptr(),
            std::ptr::from_mut(&mut reply).cast::<Message>(),
            data as *mut c_void,
        )
    };
    1
}
pub(crate) unsafe fn sender(message: *mut Message) -> *const c_char {
    unsafe { &*message.cast::<Reply>() }.sender.as_ptr()
}
pub(crate) unsafe fn payload(message: *mut Message) -> *const c_char {
    unsafe { &*message.cast::<Reply>() }.payload.as_ptr()
}
pub(crate) unsafe fn token(message: *mut Message) -> Token {
    unsafe { &*message.cast::<Reply>() }.token
}
pub(crate) unsafe fn hub_error(_: *mut Message) -> bool {
    false
}
pub(crate) unsafe fn cancel(_: *mut Handle, _: Token, _: *mut Error) -> bool {
    event();
    state(|state| state.pending = None);
    true
}
pub(crate) unsafe fn unregister(_: *mut Handle, _: *mut Error) -> bool {
    event();
    state(|state| {
        state.unregisters += 1;
        !state.unregister_failure
    })
}
pub(crate) unsafe fn context_unref(_: *mut Context) {
    event();
    state(|state| state.unrefs += 1);
}
