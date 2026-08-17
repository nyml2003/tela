//! Mobile dynamic guest 的产品装配根。
//!
//! Android 通过 bundle / guest runtime 运行该 WASM guest。iOS 不依赖它，而使用单独的
//! 静态产品装配路径，因此不会被 Wasmtime、下载或 ABI 闭包污染。

// `export_guest!` emits the audited extern "C" ABI boundary with local `allow(unsafe_code)`.
// `deny` still rejects any unsafe code written by this product outside that macro expansion.
#![deny(unsafe_code)]
#![warn(missing_docs)]

#[cfg(target_arch = "wasm32")]
mod guest;
