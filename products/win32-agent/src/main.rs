//! Win32 静态 Agent workbench 宿主入口。

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = tela_product_win32_agent::run() {
        eprintln!("tela-win32-agent-host: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("tela-win32-agent-host: 仅支持 Windows 目标");
    std::process::exit(1);
}
