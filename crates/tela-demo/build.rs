//! 构建脚本：注入构建时间戳（TELA_BUILD_TS）。
//!
//! 用途：wasm 版本标识（wasm_version() / demo_wasm_version()）——页面/ops 日志
//! 据此确认浏览器加载的是哪次构建（配合浏览器宿主的 URL 缓存破坏，根治"刷新后
//! 还是旧 wasm"的调试困惑）。
//!
//! 刷新机制：`rerun-if-changed=build.rs` 只监听自身——**ops build 每次在 cargo
//! 前 touch 本文件**（build-demo.ts），mtime 变化 → build.rs 必重跑 → 时间戳刷新。
//! （不要用 OUT_DIR marker：marker 在构建中写入，cargo 记录后永无变化 → 死锁。）

fn main() {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=TELA_BUILD_TS={}", ts);
}
