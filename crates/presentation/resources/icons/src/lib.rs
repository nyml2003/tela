//! Material Symbols Rounded 的具体图标资源实现。
//!
//! 图标语义与 provider 契约属于 `tela-contract`。本 crate 只
//! 将那些语义解析成 Material iconfont 字形和光学度量，不能被 Application 或 UI kit 当作
//! 默认图标入口。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod material;

pub use material::MaterialIconFontProvider;
