//! 资源句柄：排版样式与纹理引用（资源字节由 Presentation / Target 提供）。

/// 不透明的文字样式标识。
///
/// Kernel、UI kit 与 Application 只传递该 token，不能借此指定某个字体文件、字形 rasterizer
/// 或平台字体族。Presentation 将 token 解析为实际字体、字号策略和资源字节。
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextStyleRef(String);

impl TextStyleRef {
    /// 通用正文排版 token。
    pub const BODY: &str = "body";
    /// 图标排版 token。
    pub const ICON: &str = "icon";

    /// 创建一个产品定义的语义排版 token。
    ///
    /// 参数必须是稳定的产品语义名，例如 `body`、`icon` 或 `file-preview`，不能是字体
    /// 文件名、系统字体族或 renderer 私有句柄。具体映射只由 Presentation provider 决定。
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// 返回用于日志、wire 编码和 provider 查找的稳定语义名。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 创建正文排版 token。
    pub fn body() -> Self {
        Self::new(Self::BODY)
    }

    /// 创建图标排版 token。
    pub fn icon() -> Self {
        Self::new(Self::ICON)
    }
}

/// 纹理资源标识（`Host` 经此加载纹理）。
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureId(pub String);

/// 已加载纹理的引用，绘制命令携带此引用。
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureRef(pub String);
