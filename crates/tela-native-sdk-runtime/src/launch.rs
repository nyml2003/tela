//! Portable native-SDK command line selection.

use std::path::PathBuf;

/// A parsed command supported by every native development SDK executable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchMode {
    /// Validate a local archive without creating a platform window.
    VerifyBundle(PathBuf),
    /// Start the platform shell against a selected development bundle index.
    RunPlatform(PlatformLaunchOptions),
    /// Print command usage and exit successfully.
    Help,
}

/// Startup options consumed by a platform-owned native shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformLaunchOptions {
    /// Absolute index URL requested once during startup.
    pub bundle_index_url: String,
    /// Whether the shell emits startup/cache/runtime diagnostics to stderr.
    pub verbose: bool,
}

/// Parses native SDK launch arguments without depending on a particular window system.
pub fn launch_mode(mut args: impl Iterator<Item = String>) -> Result<LaunchMode, String> {
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
            _ => return Err(format!("unknown option: {argument}")),
        }
        current = args.next();
    }
    Ok(LaunchMode::RunPlatform(options))
}

/// Returns the local index URL associated with a native shell `--port` option.
pub fn index_url_for_port(port: u16) -> String {
    format!("http://127.0.0.1:{port}/tela-dev/latest.json")
}

/// Formats the command-line usage for a concrete SDK binary name.
pub fn usage(binary_name: &str) -> String {
    format!(
        "usage: {binary_name} [--port <1..65535> | --bundle-index <http(s) URL>] [--verbose]\\n       {binary_name} --verify-bundle <bundle.tela>"
    )
}

fn default_platform_options() -> PlatformLaunchOptions {
    PlatformLaunchOptions {
        bundle_index_url: index_url_for_port(8000),
        verbose: false,
    }
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
