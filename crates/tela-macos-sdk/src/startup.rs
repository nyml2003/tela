//! Development-bundle startup work that is safe to run away from AppKit's main thread.

use std::{
    env,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread,
};

use tela_native_sdk_runtime::{BundleLoader, BundleSource, GuestRuntime, PlatformLaunchOptions};

/// One background startup result together with the cancellation flag owned by the AppKit shell.
pub struct StartupWorker {
    /// The main thread receives the fully initialized guest from this queue.
    pub receiver: Receiver<Result<GuestRuntime, String>>,
    /// Closing the native window sets this flag before releasing UI resources.
    pub cancel: Arc<AtomicBool>,
}

/// Starts exactly one development-bundle retrieval and guest initialization worker.
pub fn start(options: PlatformLaunchOptions) -> Result<StartupWorker, String> {
    let cache_path = cache_path()?;
    if options.verbose {
        eprintln!(
            "tela-macos-sdk: startup index={} cache={}",
            options.bundle_index_url,
            cache_path.display()
        );
    }

    let (sender, receiver) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    thread::Builder::new()
        .name("tela-macos-startup".to_owned())
        .spawn(move || {
            let result = load_guest(options, cache_path);
            if !worker_cancel.load(Ordering::Acquire) {
                let _ = sender.send(result);
            }
        })
        .map_err(|error| format!("spawn macOS startup worker: {error}"))?;

    Ok(StartupWorker { receiver, cancel })
}

fn cache_path() -> Result<PathBuf, String> {
    let home = env::var_os("HOME")
        .ok_or_else(|| "HOME is required for the macOS development bundle cache".to_owned())?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Caches")
        .join("tela")
        .join("development")
        .join("last-valid.tela"))
}

fn fetch_http(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|error| format!("GET {url}: {error}"))?;
    response
        .into_body()
        .into_with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|error| format!("read {url}: {error}"))
}

fn load_guest(options: PlatformLaunchOptions, cache_path: PathBuf) -> Result<GuestRuntime, String> {
    let loader = BundleLoader::new(cache_path);
    let bundle = loader
        .load_with(&options.bundle_index_url, fetch_http)
        .map_err(|error| error.to_string())?;
    let source = match bundle.source {
        BundleSource::Network => "network",
        BundleSource::Cache => "cache fallback",
    };
    if options.verbose {
        eprintln!(
            "tela-macos-sdk: bundle={source} archive={}KB download={}ms; initializing guest",
            bundle.metrics.archive_bytes / 1024,
            bundle.metrics.download.as_millis(),
        );
        if let Some(warning) = bundle.cache_warning.as_deref() {
            eprintln!("tela-macos-sdk: bundle cache warning: {warning}");
        }
    }
    let runtime = GuestRuntime::new(&bundle.archive.app_wasm).map_err(|error| error.to_string())?;
    if options.verbose {
        eprintln!(
            "tela-macos-sdk: guest initialized compile={}ms init={}ms init_fuel={}",
            runtime.metrics().module_compile.as_millis(),
            runtime.metrics().initialize.as_millis(),
            runtime.metrics().initialize_fuel_consumed,
        );
    }
    Ok(runtime)
}
