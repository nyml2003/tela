//! Win32 surface-probe host entry point.

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = tela_product_win32_probe::run() {
        eprintln!("tela-win32-probe-host: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("tela-win32-probe-host: Windows target only");
    std::process::exit(1);
}
