//! Win32 development SDK host.
//!
//! The platform-neutral bundle loader and guest runtime are kept separate from the Win32 message
//! loop so future native SDKs reuse the same application package contract without inheriting a
//! shared window abstraction.

#![deny(unsafe_code)]
#![warn(missing_docs)]

use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[cfg(any(target_os = "windows", test))]
mod bundle_loader;
#[cfg(any(target_os = "windows", test))]
mod lifecycle;
mod runtime;

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod win32;

fn main() {
    let result = match launch_mode(env::args().skip(1)) {
        Ok(LaunchMode::VerifyBundle(path)) => verify_bundle(&path),
        Ok(LaunchMode::RunPlatform(options)) => run_platform(options),
        Ok(LaunchMode::Help) => {
            println!("{}", usage());
            Ok(())
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("tela-win32-sdk: {error}");
        std::process::exit(1);
    }
}

enum LaunchMode {
    VerifyBundle(PathBuf),
    RunPlatform(PlatformLaunchOptions),
    Help,
}

/// Startup options consumed by the platform-owned Win32 shell.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PlatformLaunchOptions {
    bundle_index_url: String,
    verbose: bool,
}

fn launch_mode(mut args: impl Iterator<Item = String>) -> Result<LaunchMode, String> {
    let Some(first) = args.next() else {
        return Ok(LaunchMode::RunPlatform(default_platform_options()));
    };
    if first == "--verify-bundle" {
        let path = args
            .next()
            .ok_or_else(|| "--verify-bundle requires a .tela archive path".to_owned())?;
        if args.next().is_some() {
            return Err("--verify-bundle accepts exactly one archive path".to_owned());
        }
        return Ok(LaunchMode::VerifyBundle(PathBuf::from(path)));
    }

    let mut options = default_platform_options();
    let mut port_was_set = false;
    let mut index_was_set = false;
    let mut current = Some(first);
    while let Some(argument) = current {
        match argument.as_str() {
            "--help" | "-h" => {
                if args.next().is_some() {
                    return Err("--help cannot be combined with startup options".to_owned());
                }
                return Ok(LaunchMode::Help);
            }
            "--verbose" => options.verbose = true,
            "--port" => {
                if port_was_set || index_was_set {
                    return Err("--port and --bundle-index are mutually exclusive".to_owned());
                }
                let value = args
                    .next()
                    .ok_or_else(|| "--port requires a value from 1 to 65535".to_owned())?;
                let port = value
                    .parse::<u16>()
                    .map_err(|_| format!("invalid --port value: {value}"))?;
                if port == 0 {
                    return Err("--port must be between 1 and 65535".to_owned());
                }
                options.bundle_index_url = index_url_for_port(port);
                port_was_set = true;
            }
            "--bundle-index" => {
                if port_was_set || index_was_set {
                    return Err("--port and --bundle-index are mutually exclusive".to_owned());
                }
                let value = args
                    .next()
                    .ok_or_else(|| "--bundle-index requires an absolute http(s) URL".to_owned())?;
                if !(value.starts_with("http://") || value.starts_with("https://")) {
                    return Err("--bundle-index must be an absolute http(s) URL".to_owned());
                }
                options.bundle_index_url = value;
                index_was_set = true;
            }
            "--verify-bundle" => {
                return Err("--verify-bundle cannot be combined with startup options".to_owned());
            }
            _ => return Err(format!("unknown option: {argument}\n{}", usage())),
        }
        current = args.next();
    }
    Ok(LaunchMode::RunPlatform(options))
}

fn default_platform_options() -> PlatformLaunchOptions {
    PlatformLaunchOptions {
        bundle_index_url: index_url_for_port(8000),
        verbose: false,
    }
}

fn index_url_for_port(port: u16) -> String {
    format!("http://127.0.0.1:{port}/tela-dev/latest.json")
}

fn usage() -> &'static str {
    "usage: tela-win32-sdk [--port <1..65535> | --bundle-index <http(s) URL>] [--verbose]\n       tela-win32-sdk --verify-bundle <bundle.tela>"
}

fn verify_bundle(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let archive = tela_bundle::read_archive(&bytes)
        .map_err(|error| format!("validate {}: {error}", path.display()))?;
    let mut runtime = runtime::GuestRuntime::new(&archive.app_wasm)
        .map_err(|error| format!("start guest runtime: {error}"))?;
    let initial_commands = runtime
        .frame()
        .map_err(|error| format!("decode verification frame: {error}"))?
        .commands
        .len();
    let input_focused = runtime.status().input_focused;
    runtime
        .dispatch(&tela_app_abi::AppEvent::Viewport {
            width: 1280.0,
            height: 720.0,
        })
        .map_err(|error| format!("dispatch verification viewport: {error}"))?;
    eprintln!(
        "tela-win32-sdk: verified bundle={}KB wasm={}KB initial_commands={} commands={} input_focused={} compile={}ms init={}ms init_fuel={} dispatch={}ms dispatch_fuel={}",
        bytes.len() / 1024,
        archive.app_wasm.len() / 1024,
        initial_commands,
        runtime
            .frame()
            .map_err(|error| format!("decode verification frame: {error}"))?
            .commands
            .len(),
        input_focused,
        runtime.metrics().module_compile.as_millis(),
        runtime.metrics().initialize.as_millis(),
        runtime.metrics().initialize_fuel_consumed,
        runtime.metrics().last_dispatch.as_millis(),
        runtime.metrics().last_dispatch_fuel_consumed,
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn run_platform(options: PlatformLaunchOptions) -> Result<(), String> {
    win32::run(options)
}

#[cfg(not(target_os = "windows"))]
fn run_platform(_options: PlatformLaunchOptions) -> Result<(), String> {
    Err("tela-win32-sdk must be built and run on Windows; use --verify-bundle for host-independent bundle validation".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{LaunchMode, launch_mode};

    #[test]
    fn parse_verify_bundle_mode_without_a_window() {
        let mode = launch_mode(["--verify-bundle".to_owned(), "demo.tela".to_owned()].into_iter())
            .expect("parse verifier mode");
        assert!(
            matches!(mode, LaunchMode::VerifyBundle(path) if path == std::path::Path::new("demo.tela"))
        );
    }

    #[test]
    fn reject_ambiguous_launch_arguments() {
        assert!(launch_mode(["unexpected".to_owned()].into_iter()).is_err());
        assert!(launch_mode(["--verify-bundle".to_owned()].into_iter()).is_err());
    }

    #[test]
    fn port_and_verbose_select_a_local_bundle_index() {
        let mode = launch_mode(
            [
                "--port".to_owned(),
                "8123".to_owned(),
                "--verbose".to_owned(),
            ]
            .into_iter(),
        )
        .expect("parse startup mode");
        assert!(matches!(mode, LaunchMode::RunPlatform(options)
            if options.verbose && options.bundle_index_url == "http://127.0.0.1:8123/tela-dev/latest.json"));
    }

    #[test]
    fn reject_conflicting_bundle_sources() {
        assert!(
            launch_mode(
                [
                    "--port".to_owned(),
                    "8000".to_owned(),
                    "--bundle-index".to_owned(),
                    "http://127.0.0.1:8001/tela-dev/latest.json".to_owned(),
                ]
                .into_iter()
            )
            .is_err()
        );
    }

    #[test]
    fn full_index_can_select_a_non_default_resource_endpoint() {
        let mode = launch_mode(
            [
                "--bundle-index".to_owned(),
                "http://192.168.1.8:8123/tela-dev/latest.json".to_owned(),
                "--verbose".to_owned(),
            ]
            .into_iter(),
        )
        .expect("parse full bundle index");
        assert!(matches!(mode, LaunchMode::RunPlatform(options)
            if options.verbose
                && options.bundle_index_url == "http://192.168.1.8:8123/tela-dev/latest.json"));
    }
}
