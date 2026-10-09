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
    criterion_platform::auxv::warm_auxiliary_vector();
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
