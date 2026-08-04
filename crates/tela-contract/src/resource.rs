//! 资源句柄：字体与纹理引用（资源字节经 `Host` 加载，见 008-交互焦点与宿主接口 4）。

/// 字体资源标识（`Host` 经此加载字体字节）。
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontRef(pub String);

/// 纹理资源标识（`Host` 经此加载纹理）。
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureId(pub String);

/// 已加载纹理的引用，绘制命令携带此引用。
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureRef(pub String);
