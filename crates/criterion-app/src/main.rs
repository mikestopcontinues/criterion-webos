// SPDX-License-Identifier: GPL-3.0-or-later
//! Native entry: main-thread SDL/GLES, cancellable async catalog, ordered input batches.
mod account;
mod artwork;
mod authentication;
mod controller;
mod jobs;
mod presentation;

mod application;
use application::Application;
use criterion_platform::{Activity, Window};
use criterion_ui::GlowRenderer;
use std::{
    ffi::CString,
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

fn main() -> ExitCode {
    prepare_process();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(stage) => {
            eprintln!("Criterion Unofficial stopped during {stage}.");
            ExitCode::FAILURE
        }
    }
}

fn prepare_process() {
    // SAFETY: this is the first application operation, before SDL, Tokio or workers.
    // No other thread can read the process environment while it is changed.
    unsafe {
        std::env::set_var("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
    }
    #[cfg(all(feature = "webos", target_arch = "arm", target_pointer_width = "32"))]
    {
        unsafe extern "C" {
            fn getauxval(kind: std::os::raw::c_ulong) -> std::os::raw::c_ulong;
        }
        // SAFETY: criterion-platform supplies the ARM32 unsigned ABI and bounded
        // one-time cache. Warm it before SDL or worker libraries initialize.
        let _ = unsafe { getauxval(6) };
    }
}

fn run() -> Result<(), &'static str> {
    let mut window = Window::open("Criterion Unofficial").map_err(|_| "the native window")?;
    // SAFETY: Window owns a current live context on this main thread. It is declared
    // before the renderer so every success/error path drops the renderer first.
    let gl = Arc::new(unsafe {
        glow::Context::from_loader_function(|name| {
            CString::new(name).map_or(std::ptr::null(), |name| window.gl_proc_address(&name))
        })
    });
    let mut painter = unsafe { GlowRenderer::new(gl) }.map_err(|_| "the GLES renderer")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| "the request runtime")?;
    let mut app = Application::new(
        window.surface().map_err(|_| "the render surface")?,
        runtime.handle(),
    )?;
    let outcome = run_loop(&mut app, &mut window, &mut painter, &runtime);
    // Every post-admission error reaches this boundary. An issued refresh/logout
    // keeps its bounded owner until settlement even when input or painting fails.
    app.background();
    let text_cleanup = window.text_input(false).map_err(|_| "text-input disposal");
    let logout_confirmed = app.finish(&runtime);
    drop(app);
    runtime.shutdown_timeout(Duration::from_secs(2));
    painter.destroy();
    if !logout_confirmed {
        return Err("remote logout confirmation");
    }
    outcome.and(text_cleanup)
}

fn run_loop(
    app: &mut Application,
    window: &mut Window,
    painter: &mut GlowRenderer,
    runtime: &tokio::runtime::Runtime,
) -> Result<(), &'static str> {
    let start = Instant::now();
    let mut activity = Activity::Foreground;

    while !app.exiting() {
        let tick = Instant::now();
        app.poll(runtime, activity == Activity::Foreground);
        // Bound SDL drain work even when a source continuously produces events. Key
        // boundaries flush earlier pointer/IME events before applying the remote action.
        // No input kind can reorder a click/text commit across a navigation transition.
        for _ in 0..128 {
            let Some(event) = window.poll_event().map_err(|_| "native input")? else {
                break;
            };
            let surface = window.surface().map_err(|_| "the render surface")?;
            app.event(event, surface, runtime, start.elapsed());
        }
        let next_activity = window.activity();
        if next_activity != activity {
            if next_activity == Activity::Foreground {
                app.foreground(runtime.handle());
            } else {
                app.background();
            }
            activity = next_activity;
        }
        // CPU processing remains admitted on background/closure, delivering releases
        // into egui. Retain texture deltas for the first actual foreground repaint.
        app.consume(runtime, start.elapsed());
        if activity == Activity::Closed {
            app.exit();
        }
        if activity == Activity::Foreground && !app.exiting() {
            if let Some(mut output) = app.take_output() {
                let surface = window.surface().map_err(|_| "the render surface")?;
                painter
                    .paint(
                        [surface.drawable.width, surface.drawable.height],
                        app.context(),
                        &mut output,
                    )
                    .map_err(|_| "the render frame")?;
                window.present().map_err(|_| "native presentation")?;
            }
            // Focus can legitimately disappear between input and this request. Input
            // ownership remains correct; the next foreground tick retries native IME.
            match window.text_input(app.wants_text_input()) {
                Ok(()) | Err(criterion_platform::PlatformError::KeyboardFocusUnavailable) => (),
                Err(_) => return Err("native text input"),
            }
        }
        let minimum = if activity == Activity::Foreground {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(100)
        };
        if let Some(rest) = minimum.checked_sub(tick.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    Ok(())
}
