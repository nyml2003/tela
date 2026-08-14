//! `tela` 的图标语义、来源抽象与光学对齐。
//!
//! `tela-core` 只消费普通文本、图片和几何原语；本 crate 将图标语义解析为这些已有原语，
//! 并用受控字体的实际墨迹度量写入纯视觉 offset。这样 iconfont、未来 SVG 或平台图标都
//! 可以替换 provider，而不让布局、命中、焦点身份或 renderer 协议产生图标特例。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod icon;
mod key;
mod material;
mod provider;

pub use icon::Icon;
pub use key::IconKey;
pub use material::{IconName, MaterialIconFontProvider};
pub use provider::{IconOpticalMetrics, IconProvider, IconRequest, IconResolveError, IconVisual};
