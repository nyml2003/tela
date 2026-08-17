//! 稳定组件路径类型。

use std::fmt;

/// Application 为组件实例分配的稳定路径。
///
/// 此路径不等同于 Kernel 的 NodeId 或业务字段 BindId。它用来表达一个组件实例在
/// Application 视图组合中的位置，并作为显式 Signal 观察关系的所有者。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct ComponentPath(String);

impl ComponentPath {
    /// 以调用方提供的稳定字符串创建路径。
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// 返回路径文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 构造根组件下一个具名部件的路径。
    pub fn part(&self, part: impl AsRef<str>) -> ComponentPartPath {
        ComponentPartPath(format!("{}.{}", self.0, part.as_ref()))
    }
}

impl From<String> for ComponentPath {
    fn from(path: String) -> Self {
        Self::new(path)
    }
}

impl From<&str> for ComponentPath {
    fn from(path: &str) -> Self {
        Self::new(path)
    }
}

impl AsRef<str> for ComponentPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ComponentPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// 一个具体 Root/Part 在组件树中的稳定语义路径。
///
/// 它由组件路径、部件角色和可选的稳定 item key 组成。EventRegistry 以此将帧内
/// UiAction 路由为组件域事件。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct ComponentPartPath(String);

impl ComponentPartPath {
    /// 直接创建一个已完整编码的部件路径。
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// 返回路径文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 为可重排集合中的一个稳定 item key 构造子路径。
    pub fn item(&self, key: impl AsRef<str>) -> Self {
        Self(format!(r#"{}["{}"]"#, self.0, key.as_ref()))
    }

    /// 返回末尾 item 片段中的稳定 key。
    ///
    /// 这是 Application 在收到由 Self::item 生成的 RoutedEvent 后，把部件路径映射回
    /// 自己的稳定实体 key 的入口；没有 item 后缀的普通 Part 返回 None。
    pub fn item_key(&self) -> Option<&str> {
        self.0
            .rsplit_once(r#"[""#)
            .and_then(|(_, key)| key.strip_suffix(r#""]"#))
    }
}

impl AsRef<str> for ComponentPartPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ComponentPartPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::ComponentPath;

    #[test]
    fn component_and_item_paths_keep_their_semantic_segments() {
        let root = ComponentPath::new("settings.tabs");
        let trigger = root.part("trigger");
        assert_eq!(
            trigger.item("appearance").as_str(),
            r#"settings.tabs.trigger["appearance"]"#
        );
        assert_eq!(trigger.item("appearance").item_key(), Some("appearance"));
        assert_eq!(trigger.item_key(), None);
    }
}
