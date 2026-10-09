mod bus;
mod ffi;

use bus::{Bus, Method, Reply};
use criterion_native_caller_probe::{CloseWait, KeyAdmission};
use criterion_platform::{Activity, Event, Lifecycle, Window};
use glow::HasContext;
use std::time::{Duration, Instant};

const REQUEST_MS: u64 = 3000;
const ABSOLUTE_MS: u64 = 20_000;

struct Pump {
    // Struct fields drop in declaration order, including early-return cleanup.
    gl: glow::Context,
    window: Window,
    origin: Instant,
    stopped: bool,
}
impl Pump {
    fn now(&self) -> u64 {
        self.origin.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    fn deadline(&self) -> u64 {
        self.now().saturating_add(REQUEST_MS).min(ABSOLUTE_MS)
    }
    fn tick(&mut self, bus: &mut Bus) {
        for _ in 0..64 {
            match self.window.poll_event() {
                Ok(Some(
                    Event::Quit
                    | Event::Lifecycle(Lifecycle::WillBackground | Lifecycle::Background),
                )) => self.stopped = true,
                Ok(Some(Event::Key(key))) if key.pressed && matches!(key.scancode, 41 | 482) => {
                    self.stopped = true
                }
                Ok(Some(_)) => (),
                Ok(None) => break,
                Err(_) => {
                    self.stopped = true;
                    break;
                }
            }
            if self.now() >= ABSOLUTE_MS {
                break;
            }
        }
        for _ in 0..32 {
            if self.now() >= ABSOLUTE_MS || !bus.dispatch_one() {
                break;
            }
        }
        if !self.stopped && self.window.activity() == Activity::Foreground {
            // This disposable app uses only a fixed status background, no account content.
            unsafe {
                self.gl.clear_color(0.035, 0.05, 0.075, 1.0);
                self.gl.clear(glow::COLOR_BUFFER_BIT);
            }
            if self.window.present().is_err() {
                self.stopped = true;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    fn wait(&mut self, bus: &mut Bus, method: Method) -> Option<Reply> {
        let deadline = self.deadline();
        while !self.stopped && self.now() < deadline {
            self.tick(bus);
            if self.stopped || self.now() >= deadline {
                return None;
            }
            if let Some(reply) = bus.take(method) {
                return reply.ok();
            }
        }
        None
    }
}

/// Entry owns foreground SDL and the private GLib context on the same main thread.
pub fn run() -> bool {
    let window = match Window::open("Criterion Native Caller Probe") {
        Ok(window) => window,
        Err(_) => {
            eprintln!("native-caller: window-unavailable");
            return false;
        }
    };
    let gl =
        unsafe { glow::Context::from_loader_function_cstr(|name| window.gl_proc_address(name)) };
    let version = unsafe { gl.get_parameter_string(glow::VERSION) };
    if !version.starts_with("OpenGL ES 2.0") {
        eprintln!("native-caller: gles2-unavailable");
        return false;
    }
    let mut pump = Pump {
        window,
        gl,
        origin: Instant::now(),
        stopped: false,
    };
    let mut bus = match Bus::open() {
        Ok(bus) => bus,
        Err(stage) => {
            eprintln!("native-caller: {stage}");
            return false;
        }
    };
    eprintln!("native-caller: named-registration-admitted");
    if !bus.call(Method::Attach) {
        let clean = bus.dispose();
        eprintln!("native-caller: attach-not-issued bus-cleanup={clean}");
        return false;
    }
    let initial = match pump.wait(&mut bus, Method::Attach) {
        Some(Reply::Bridge(reply)) if reply.running() => Some(reply),
        _ => None,
    };
    if initial.is_none() {
        eprintln!("native-caller: attach-unconfirmed");
    }
    let mut liveness = false;
    let mut acknowledged = initial;
    if let Some(initial) = initial
        && !pump.stopped
        && bus.call(Method::Ping)
        && let Some(Reply::Bridge(reply)) = pump.wait(&mut bus, Method::Ping)
        && reply.pinged_after(initial)
    {
        liveness = true;
        acknowledged = Some(reply);
    }
    if !liveness {
        eprintln!("native-caller: ping-unconfirmed");
    }
    // Once attach is issued, attempt fixed close even if admission/initial reply failed.
    // A stop/background event prevents new discovery, but cannot abandon issued close.
    let mut close = CloseWait::new(acknowledged, pump.deadline());
    if bus.call(Method::Close) {
        while !close.complete(pump.now()) {
            pump.tick(&mut bus);
            if let Some(reply) = bus.take(Method::Close) {
                close.observe(
                    pump.now(),
                    reply.and_then(|reply| match reply {
                        Reply::Bridge(snapshot) => Ok(snapshot),
                        Reply::Keymanager(_) => {
                            Err(criterion_native_caller_probe::Failure::InvalidResponse)
                        }
                    }),
                );
            }
        }
    }
    if !close.retired() {
        eprintln!("native-caller: close-unconfirmed");
    }
    let mut key_read = false;
    if liveness && close.retired() && !pump.stopped && bus.call(Method::Keymanager) {
        match pump.wait(&mut bus, Method::Keymanager) {
            Some(Reply::Keymanager(KeyAdmission::MissingKey)) => {
                key_read = true;
                eprintln!("native-caller: keymanager-read-only-admitted");
            }
            Some(Reply::Keymanager(KeyAdmission::ExistingPublicKey)) => {
                eprintln!("native-caller: key-name-already-present")
            }
            _ => eprintln!("native-caller: keymanager-read-only-unconfirmed"),
        }
    }
    let clean = bus.dispose();
    eprintln!(
        "native-caller: liveness={liveness} broker-retired={} bus-cleanup={clean} interrupted={}",
        close.retired(),
        pump.stopped
    );
    // Drop the GL object before SDL destroys its context. No textures or programs are allocated.
    drop(pump.gl);
    drop(pump.window);
    liveness && close.retired() && clean && key_read && !pump.stopped
}
