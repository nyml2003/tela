//! Win32 静态文本编辑器宿主入口。

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = tela_product_win32_editor::run() {
        eprintln!("tela-win32-editor-host: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("tela-win32-editor-host: 仅支持 Windows 目标");
    std::process::exit(1);
}
