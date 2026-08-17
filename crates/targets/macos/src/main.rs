//! macOS development SDK host.
//!
//! The binary shares package/runtime protocol code with the Win32 SDK but owns an AppKit event
//! loop, native input normalization, a Metal-backed WGPU surface, and its macOS cache location.

#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::env;

use tela_desktop_runtime::{LaunchMode, PlatformLaunchOptions, launch_mode, usage, verify_bundle};

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod appkit;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod gpu;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod input;
#[cfg(target_os = "macos")]
mod startup;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod view;

fn main() {
    let result = match launch_mode(env::args().skip(1)) {
        Ok(LaunchMode::VerifyBundle(path)) => verify_bundle(&path).map(|verification| {
            eprintln!("tela-macos-host: {verification}");
        }),
        Ok(LaunchMode::RunPlatform(options)) => run_platform(options),
        Ok(LaunchMode::Help) => {
            println!("{}", usage("tela-macos-host"));
            Ok(())
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("tela-macos-host: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
fn run_platform(options: PlatformLaunchOptions) -> Result<(), String> {
    appkit::run(options)
}

#[cfg(not(target_os = "macos"))]
fn run_platform(_options: PlatformLaunchOptions) -> Result<(), String> {
    Err("tela-macos-host must be built and run on macOS; use --verify-bundle for host-independent bundle validation".to_owned())
}
