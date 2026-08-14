//! 与具体图标来源无关的图标语义键。

/// 图标的稳定语义键。
///
/// 应用和 `tela-ui` 只表达此键或 [`crate::IconName`]，不依赖 iconfont 码位、SVG 文件名或
/// 某个 renderer 的资源句柄。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IconKey(String);

impl IconKey {
    /// 创建图标语义键。
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 返回键的字符串表示。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for IconKey {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for IconKey {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}
