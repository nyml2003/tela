//! 变速齿轮宿主入口。

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = tela_product_speed_gear::run() {
        eprintln!("tela-speed-gear-host: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("tela-speed-gear-host: 仅支持 Windows x64");
    std::process::exit(1);
}
