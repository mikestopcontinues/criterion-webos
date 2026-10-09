#[cfg(all(
    target_os = "linux",
    target_arch = "arm",
    target_pointer_width = "32",
    target_endian = "little"
))]
mod native;

#[cfg(all(
    target_os = "linux",
    target_arch = "arm",
    target_pointer_width = "32",
    target_endian = "little"
))]
fn main() {
    // These are the first application operations, before SDL or any worker can exist.
    unsafe {
        std::env::set_var("SDL_WEBOS_ACCESS_POLICY_KEYS_BACK", "true");
    }
    unsafe extern "C" {
        fn getauxval(kind: std::ffi::c_ulong) -> std::ffi::c_ulong;
    }
    // Warm criterion-platform's bounded one-time cache before native libraries create threads.
    let _ = unsafe { getauxval(6) };
    if !native::run() {
        std::process::exit(1);
    }
}

#[cfg(not(all(
    target_os = "linux",
    target_arch = "arm",
    target_pointer_width = "32",
    target_endian = "little"
)))]
fn main() {
    eprintln!("native-caller: unsupported-host");
    std::process::exit(1);
}
