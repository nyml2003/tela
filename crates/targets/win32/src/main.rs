//! Win32 development SDK host.
//!
//! The platform-neutral runtime lives in `tela-desktop-runtime`; this crate owns only the
//! Windows window, input, HTTP client, local cache path, and WGPU presentation lifecycle.

#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::env;

use tela_desktop_runtime::{LaunchMode, PlatformLaunchOptions, launch_mode, usage, verify_bundle};

fn main() {
    let result = match launch_mode(env::args().skip(1)) {
        Ok(LaunchMode::VerifyBundle(path)) => verify_bundle(&path).map(|verification| {
            eprintln!("tela-win32-host: {verification}");
        }),
        Ok(LaunchMode::RunPlatform(options)) => run_platform(options),
        Ok(LaunchMode::Help) => {
            println!("{}", usage("tela-win32-host"));
            Ok(())
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("tela-win32-host: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "windows")]
fn run_platform(options: PlatformLaunchOptions) -> Result<(), String> {
    tela_target_win32::run_sdk_window(options)
}

#[cfg(not(target_os = "windows"))]
fn run_platform(_options: PlatformLaunchOptions) -> Result<(), String> {
    Err("tela-win32-host must be built and run on Windows; use --verify-bundle for host-independent bundle validation".to_owned())
}
