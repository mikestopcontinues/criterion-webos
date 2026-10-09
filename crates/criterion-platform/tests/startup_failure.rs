// SPDX-License-Identifier: GPL-3.0-or-later
// Even a target with no host startup cases needs the shared runtime for its test harness.
extern crate criterion_platform as _;

#[cfg(all(feature = "sdl", not(feature = "webos")))]
mod host {
    use criterion_platform::{PlatformError, Window};
    use std::process::Command;

    #[link(name = "SDL2")]
    unsafe extern "C" {
        fn SDL_WasInit(flags: u32) -> u32;
        fn SDL_InitSubSystem(flags: u32) -> i32;
        fn SDL_QuitSubSystem(flags: u32);
    }

    #[test]
    fn failed_startup_releases_video_dependencies_for_other_sdl_users() {
        const CHILD: &str = "CRITERION_PLATFORM_STARTUP_FAILURE_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let result = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "host::failed_startup_releases_video_dependencies_for_other_sdl_users",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env(
                    "SDL_VIDEODRIVER",
                    "criterion-intentionally-unavailable-driver",
                )
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
        // A fresh subprocess gives this test exclusive SDL ownership, with a nonexistent
        // video driver: no window, graphics context, display server or GPU is accessed.
        for _ in 0..2 {
            assert!(matches!(
                Window::open("Criterion Unofficial"),
                Err(PlatformError::Sdl { .. })
            ));
            assert_eq!(
                unsafe { SDL_WasInit(0) },
                0,
                "a failed constructor must not retain another subsystem"
            );
        }
    }

    #[test]
    fn existing_sdl_events_are_rejected_without_releasing_their_owner() {
        const CHILD: &str = "CRITERION_PLATFORM_EXISTING_SDL_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let result = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "host::existing_sdl_events_are_rejected_without_releasing_their_owner",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env(
                    "SDL_VIDEODRIVER",
                    "criterion-intentionally-unavailable-driver",
                )
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
        // EVENTS initializes no video driver. The nonexistent video driver also prevents
        // an erroneous constructor from accessing a display server or graphics device.
        assert_eq!(unsafe { SDL_InitSubSystem(0x4000) }, 0);
        for _ in 0..2 {
            let error = Window::open("Criterion Unofficial").err().unwrap();
            assert!(matches!(error, PlatformError::AlreadyInitialized));
            assert_eq!(unsafe { SDL_WasInit(0) }, 0x4000);
        }
        unsafe {
            SDL_QuitSubSystem(0x4000);
        }
        assert_eq!(unsafe { SDL_WasInit(0) }, 0);
    }
}
