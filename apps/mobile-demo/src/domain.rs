//! Static mobile file-browser data. This is intentionally independent from `tela-desktop-demo`.

/// Mobile file entry kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// A navigable directory.
    Folder,
    /// A text-like document with a static preview.
    Document,
    /// A generic asset represented by metadata only in the first mobile proof.
    Asset,
}

/// One immutable mock workspace item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Stable application-level identity.
    pub id: &'static str,
    /// Parent folder identity; `None` is the workspace root.
    pub parent: Option<&'static str>,
    /// User-visible name.
    pub name: &'static str,
    /// Semantic kind.
    pub kind: EntryKind,
    /// Short metadata line.
    pub metadata: &'static str,
    /// Static text-only preview content when available.
    pub preview: Option<&'static str>,
}

/// Immutable mock workspace for the mobile proof.
#[derive(Clone, Debug)]
pub struct MobileWorkspace {
    entries: Vec<Entry>,
}

impl Default for MobileWorkspace {
    fn default() -> Self {
        Self::sample()
    }
}

impl MobileWorkspace {
    /// Creates the fixed workspace used by the first Android bundle.
    pub fn sample() -> Self {
        Self {
            entries: vec![
                Entry {
                    id: "design",
                    parent: None,
                    name: "设计资料",
                    kind: EntryKind::Folder,
                    metadata: "12 个项目",
                    preview: None,
                },
                Entry {
                    id: "notes",
                    parent: None,
                    name: "工作笔记",
                    kind: EntryKind::Folder,
                    metadata: "6 个项目",
                    preview: None,
                },
                Entry {
                    id: "readme",
                    parent: None,
                    name: "README.md",
                    kind: EntryKind::Document,
                    metadata: "今天 09:42 · 4.2 KB",
                    preview: Some(
                        "Tela Mobile\n\n这是一个独立的移动端文件浏览器。\n它复用布局、焦点和帧协议，\n但不复用桌面的页面结构。",
                    ),
                },
                Entry {
                    id: "architecture",
                    parent: Some("design"),
                    name: "架构迭代方案.md",
                    kind: EntryKind::Document,
                    metadata: "昨天 · 18.6 KB",
                    preview: Some(
                        "绝对一致 -> 复用代码\n语义一致 -> 复用协议\n概念一致 -> 只共享抽象边界",
                    ),
                },
                Entry {
                    id: "wireframes",
                    parent: Some("design"),
                    name: "移动端线框图",
                    kind: EntryKind::Asset,
                    metadata: "昨天 · 2.1 MB",
                    preview: None,
                },
                Entry {
                    id: "research",
                    parent: Some("design"),
                    name: "调研",
                    kind: EntryKind::Folder,
                    metadata: "4 个项目",
                    preview: None,
                },
                Entry {
                    id: "android-notes",
                    parent: Some("notes"),
                    name: "Android 调试记录.txt",
                    kind: EntryKind::Document,
                    metadata: "周一 · 2.8 KB",
                    preview: Some(
                        "验收重点\n\n1. Bundle 网络严格校验\n2. Vulkan surface 生命周期\n3. 中文 IME 全值同步\n4. 单指滚动与系统返回",
                    ),
                },
                Entry {
                    id: "ideas",
                    parent: Some("notes"),
                    name: "想法收集.md",
                    kind: EntryKind::Document,
                    metadata: "上周 · 1.4 KB",
                    preview: Some("先让真实应用出现，再抽取真正相同的部分。"),
                },
                Entry {
                    id: "field-notes",
                    parent: Some("research"),
                    name: "终端形态观察.md",
                    kind: EntryKind::Document,
                    metadata: "8 月 12 日 · 6.4 KB",
                    preview: Some("TUI 是边界验证题，不是当前产品路线。"),
                },
            ],
        }
    }

    /// Finds one entry by its stable identity.
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Returns direct children for a folder, in their stable product order.
    pub fn children(&self, folder: Option<&str>) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.parent == folder)
            .collect()
    }

    /// Searches the workspace by a simple case-insensitive name match.
    pub fn search(&self, query: &str) -> Vec<&Entry> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        let needle = query.to_lowercase();
        self.entries
            .iter()
            .filter(|entry| entry.name.to_lowercase().contains(&needle))
            .collect()
    }

    /// Returns the title shown for one folder identity.
    pub fn folder_title(&self, folder: Option<&str>) -> &'static str {
        folder
            .and_then(|id| self.entry(id))
            .map(|entry| entry.name)
            .unwrap_or("我的文件")
    }
}

#[cfg(test)]
mod tests {
    use super::MobileWorkspace;

    #[test]
    fn search_crosses_folder_boundaries_without_reusing_desktop_data() {
        let workspace = MobileWorkspace::sample();
        let names: Vec<_> = workspace
            .search("架构")
            .into_iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(names, ["architecture"]);
    }
}
