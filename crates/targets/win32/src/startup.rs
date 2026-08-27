//! Background 启动轴：后台线程下载/校验 bundle 并编译 WASM guest。
//!
//! UI 线程在等待期间保持响应（移动/关闭/焦点），先绘制 Loading 页；worker 完成后经
//! `WM_TELA_STARTUP_READY` 投递交接。

#![allow(unsafe_code)]

use std::{
    env,
    ffi::c_void,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread,
};

use tela_desktop_runtime::{BundleLoader, BundleSource, GuestRuntime, PlatformLaunchOptions};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// worker 完成后投递到 UI 线程的私有消息。
pub(crate) const WM_TELA_STARTUP_READY: u32 = WM_APP + 1;

/// 设备丢失回调线程投递到 UI 线程的私有消息。
pub(crate) const WM_TELA_DEVICE_LOST: u32 = WM_APP + 2;

/// bundle 后台加载结果通道。
pub(crate) type StartupChannel = (
    Sender<Result<GuestRuntime, String>>,
    Receiver<Result<GuestRuntime, String>>,
);

pub(crate) fn startup_channel() -> StartupChannel {
    mpsc::channel()
}

pub(crate) fn cache_path() -> Result<PathBuf, String> {
    let root = env::var_os("LOCALAPPDATA")
        .or_else(|| env::var_os("TEMP"))
        .ok_or_else(|| {
            "LOCALAPPDATA or TEMP is required for development bundle cache".to_owned()
        })?;
    Ok(PathBuf::from(root)
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
            "tela-win32-host: bundle={source} archive={}KB download={}ms; initializing guest",
            bundle.metrics.archive_bytes / 1024,
            bundle.metrics.download.as_millis(),
        );
        if let Some(warning) = bundle.cache_warning.as_deref() {
            eprintln!("tela-win32-host: bundle cache warning: {warning}");
        }
    }
    let runtime = GuestRuntime::new(&bundle.archive.app_wasm).map_err(|error| error.to_string())?;
    if options.verbose {
        eprintln!(
            "tela-win32-host: guest initialized compile={}ms init={}ms init_fuel={}",
            runtime.metrics().module_compile.as_millis(),
            runtime.metrics().initialize.as_millis(),
            runtime.metrics().initialize_fuel_consumed,
        );
    }
    Ok(runtime)
}

/// 启动一个后台 worker；`start_ready_message` 是完成通知消息 ID。
pub(crate) fn spawn_startup_worker(
    options: PlatformLaunchOptions,
    cache_path: PathBuf,
    sender: Sender<Result<GuestRuntime, String>>,
    cancel: Arc<AtomicBool>,
    hwnd: HWND,
) -> Result<(), String> {
    let hwnd_bits = hwnd.0 as isize;
    thread::Builder::new()
        .name("tela-win32-startup".to_owned())
        .spawn(move || {
            let result = load_guest(options, cache_path);
            if cancel.load(Ordering::Acquire) {
                return;
            }
            if sender.send(result).is_ok() && !cancel.load(Ordering::Acquire) {
                let hwnd = HWND(hwnd_bits as *mut c_void);
                // SAFETY: the notification carries no pointer. If the HWND was destroyed first,
                // PostMessageW fails and the receiver/result are simply dropped by the worker.
                let _ = unsafe {
                    PostMessageW(
                        Some(hwnd),
                        WM_TELA_STARTUP_READY,
                        WPARAM::default(),
                        LPARAM::default(),
                    )
                };
            }
        })
        .map(|_| ())
        .map_err(|error| format!("spawn startup worker: {error}"))
}

/// worker 结果的非阻塞读取；通道断开视为启动失败。
pub(crate) enum StartupPoll {
    Pending,
    Ready(Result<GuestRuntime, String>),
    Disconnected,
}

pub(crate) fn poll_startup(
    receiver: &mut Option<Receiver<Result<GuestRuntime, String>>>,
) -> StartupPoll {
    let Some(rx) = receiver.as_ref() else {
        return StartupPoll::Disconnected;
    };
    match rx.try_recv() {
        Ok(result) => {
            let _ = receiver.take();
            StartupPoll::Ready(result)
        }
        Err(TryRecvError::Empty) => StartupPoll::Pending,
        Err(TryRecvError::Disconnected) => {
            let _ = receiver.take();
            StartupPoll::Disconnected
        }
    }
}
