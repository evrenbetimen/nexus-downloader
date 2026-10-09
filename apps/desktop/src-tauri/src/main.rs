// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Browsers start this same executable as the extension's native messaging host.
    let args: Vec<_> = std::env::args_os().collect();
    if nexus_bridge::host::is_host_invocation(&args) {
        std::process::exit(nexusdl_lib::run_native_host());
    }
    nexusdl_lib::run();
}
