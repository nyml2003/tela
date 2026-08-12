//! 文件管理器演示的纯业务 Model 与 Controller。
//!
//! 这里不依赖 tela 类型：文件实体、当前会话和可撤销命令属于宿主业务；View 只消费快照。

use std::collections::BTreeSet;

/// 业务稳定身份，不是 tela 节点 key。
pub type EntryId = u32;

/// 文件或目录的类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Folder,
    Text,
    Image,
    Archive,
}

/// 内存工作区的一项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: EntryId,
    pub parent: Option<EntryId>,
    pub name: String,
    pub kind: EntryKind,
    pub bytes: u64,
    pub modified: &'static str,
    pub favorite: bool,
    pub tags: Vec<String>,
    pub trashed: bool,
    pub text: Option<&'static str>,
}

/// 详情区的目录视图方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryView {
    List,
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    Name,
    Modified,
    Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryFilter {
    All,
    Favorites,
    Tagged,
    Trash,
}

/// 需要用户确认或填写内容的文件操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationKind {
    NewFolder,
    Rename,
    MoveToDesign,
    AddTag,
    Trash,
}

/// Modal 的受控草稿；它属于会话，不是文件实体的事实来源。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationDraft {
    pub kind: OperationKind,
    pub value: String,
}

/// 可由工具栏或组件意图触发的业务命令。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileCommand {
    NewFolderNamed(String),
    RenameSelectedAs(String),
    CopySelected,
    MoveSelectedTo(EntryId),
    TrashSelected,
    RestoreSelected,
    ToggleFavorite,
    ToggleView,
    ToggleSort,
    ToggleFilter,
    AddTagNamed(String),
    Undo,
}

/// 文件管理器业务模型：唯一的实体事实来源。
#[derive(Clone, Debug)]
pub struct FileManagerModel {
    entries: Vec<Entry>,
    next_id: EntryId,
    undo: Vec<Vec<Entry>>,
}

/// 只属于当前页面的会话状态。
#[derive(Clone, Debug)]
pub struct FileManagerSession {
    pub current_dir: EntryId,
    pub selected: BTreeSet<EntryId>,
    pub query: String,
    pub view: DirectoryView,
    pub show_navigation: bool,
    pub sort: SortMode,
    pub filter: EntryFilter,
    pub operation: Option<OperationDraft>,
    pub notice: String,
}

impl FileManagerModel {
    pub fn sample() -> Self {
        Self {
            entries: vec![
                folder(1, None, "工作区", "今天 09:42"),
                folder(2, Some(1), "设计", "昨天 18:22"),
                folder(3, Some(1), "源码", "今天 09:18"),
                folder(4, Some(1), "归档", "2026-08-01"),
                file(
                    5,
                    Some(1),
                    "README.md",
                    EntryKind::Text,
                    8_241,
                    "今天 09:40",
                    Some(
                        "# Tela 工作区\n\n这里是文件管理器演示的只读说明。\n\n## 当前工作\n\n- 完成响应式组件运行时\n- 验证 raster 和 WGPU\n- 整理交互回归\n",
                    ),
                ),
                file(
                    6,
                    Some(1),
                    "roadmap.rs",
                    EntryKind::Text,
                    12_482,
                    "今天 08:51",
                    Some(
                        "pub struct Roadmap {\n    pub title: String,\n    pub completed: usize,\n}\n\nimpl Roadmap {\n    pub fn next(&self) -> &str {\n        \"finish file manager demo\"\n    }\n}\n",
                    ),
                ),
                file(
                    7,
                    Some(1),
                    "hero.png",
                    EntryKind::Image,
                    2_481_104,
                    "昨天 16:33",
                    None,
                ),
                file(
                    8,
                    Some(2),
                    "tokens.json",
                    EntryKind::Text,
                    3_921,
                    "昨天 17:02",
                    Some("{\n  \"primary\": \"#2563EB\",\n  \"surface\": \"#FFFFFF\"\n}\n"),
                ),
                file(
                    9,
                    Some(2),
                    "icons.svg",
                    EntryKind::Image,
                    98_114,
                    "昨天 15:20",
                    None,
                ),
                file(
                    10,
                    Some(3),
                    "main.rs",
                    EntryKind::Text,
                    18_230,
                    "今天 09:18",
                    Some("fn main() {\n    println!(\"tela file manager\");\n}\n"),
                ),
                file(
                    11,
                    Some(3),
                    "layout.rs",
                    EntryKind::Text,
                    21_804,
                    "今天 08:47",
                    Some("pub fn layout() {\n    // deterministic layout\n}\n"),
                ),
                file(
                    12,
                    Some(4),
                    "release-2026-08.zip",
                    EntryKind::Archive,
                    4_983_121,
                    "2026-08-01",
                    None,
                ),
            ],
            next_id: 13,
            undo: Vec::new(),
        }
    }

    pub fn entry(&self, id: EntryId) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|entry| entry.id == id && !entry.trashed)
    }

    pub fn entries_in_filtered(
        &self,
        parent: EntryId,
        query: &str,
        filter: EntryFilter,
        sort: SortMode,
    ) -> Vec<&Entry> {
        let query = query.trim().to_lowercase();
        let mut entries: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|entry| {
                let visible = match filter {
                    EntryFilter::All => !entry.trashed,
                    EntryFilter::Favorites => !entry.trashed && entry.favorite,
                    EntryFilter::Tagged => !entry.trashed && !entry.tags.is_empty(),
                    EntryFilter::Trash => entry.trashed,
                };
                let in_scope = match filter {
                    EntryFilter::All => entry.parent == Some(parent),
                    EntryFilter::Favorites | EntryFilter::Tagged | EntryFilter::Trash => true,
                };
                visible
                    && in_scope
                    && (query.is_empty() || entry.name.to_lowercase().contains(&query))
            })
            .collect();
        match sort {
            SortMode::Name => entries
                .sort_by_key(|entry| (entry.kind != EntryKind::Folder, entry.name.to_lowercase())),
            SortMode::Modified => entries.sort_by_key(|entry| entry.modified),
            SortMode::Size => entries.sort_by_key(|entry| entry.bytes),
        }
        entries
    }

    pub fn folders(&self) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| !entry.trashed && entry.kind == EntryKind::Folder)
            .collect()
    }

    pub fn apply(&mut self, session: &mut FileManagerSession, command: FileCommand) {
        match command {
            FileCommand::Undo => {
                if let Some(previous) = self.undo.pop() {
                    self.entries = previous;
                    session.notice = "已撤销上一步操作".to_owned();
                }
                return;
            }
            FileCommand::ToggleView => {
                session.view = match session.view {
                    DirectoryView::List => DirectoryView::Grid,
                    DirectoryView::Grid => DirectoryView::List,
                };
                session.notice = match session.view {
                    DirectoryView::List => "已切换到详细列表",
                    DirectoryView::Grid => "已切换到缩略图网格",
                }
                .to_owned();
                return;
            }
            FileCommand::ToggleSort => {
                session.sort = match session.sort {
                    SortMode::Name => SortMode::Modified,
                    SortMode::Modified => SortMode::Size,
                    SortMode::Size => SortMode::Name,
                };
                session.notice = "已切换排序方式".to_owned();
                return;
            }
            FileCommand::ToggleFilter => {
                session.filter = match session.filter {
                    EntryFilter::All => EntryFilter::Favorites,
                    EntryFilter::Favorites => EntryFilter::Tagged,
                    EntryFilter::Tagged => EntryFilter::Trash,
                    EntryFilter::Trash => EntryFilter::All,
                };
                session.notice = "已切换筛选范围".to_owned();
                return;
            }
            _ => {}
        }
        let before = self.entries.clone();
        let changed = match command {
            FileCommand::NewFolderNamed(_) => {
                let id = self.next_id;
                self.next_id += 1;
                let name = match command {
                    FileCommand::NewFolderNamed(name) if !name.trim().is_empty() => name,
                    _ => format!("新建文件夹 {id}"),
                };
                self.entries
                    .push(folder(id, Some(session.current_dir), &name, "刚刚"));
                session.selected = BTreeSet::from([id]);
                session.notice = "已新建文件夹".to_owned();
                true
            }
            FileCommand::RenameSelectedAs(name) => self.rename_selected_as(session, name),
            FileCommand::CopySelected => self.copy_selected(session),
            FileCommand::MoveSelectedTo(target) => self.move_selected(session, target),
            FileCommand::TrashSelected => self.trash_selected(session),
            FileCommand::RestoreSelected => self.restore_selected(session),
            FileCommand::ToggleFavorite => self.toggle_favorite(session),
            FileCommand::AddTagNamed(tag) => self.add_tag_named(session, tag),
            FileCommand::Undo
            | FileCommand::ToggleView
            | FileCommand::ToggleSort
            | FileCommand::ToggleFilter => false,
        };
        if changed {
            self.undo.push(before);
        }
    }

    fn rename_selected_as(&mut self, session: &mut FileManagerSession, name: String) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let Some(&id) = session.selected.iter().next() else {
            return false;
        };
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == id && !entry.trashed)
        else {
            return false;
        };
        entry.name = name.to_owned();
        session.notice = "已重命名选中项目".to_owned();
        true
    }

    fn copy_selected(&mut self, session: &mut FileManagerSession) -> bool {
        let Some(&id) = session.selected.iter().next() else {
            return false;
        };
        let Some(source) = self.entry(id).cloned() else {
            return false;
        };
        let id = self.next_id;
        self.next_id += 1;
        let mut copy = source;
        copy.id = id;
        copy.name = format!("{} 副本", copy.name);
        copy.modified = "刚刚";
        self.entries.push(copy);
        session.selected = BTreeSet::from([id]);
        session.notice = "已复制选中项目".to_owned();
        true
    }

    fn move_selected(&mut self, session: &mut FileManagerSession, target: EntryId) -> bool {
        if target == session.current_dir || self.entry(target).is_none() {
            return false;
        }
        let mut changed = false;
        for id in &session.selected {
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.id == *id && !entry.trashed)
            {
                entry.parent = Some(target);
                changed = true;
            }
        }
        if changed {
            session.selected.clear();
            session.notice = "已移动到设计目录".to_owned();
        }
        changed
    }

    fn trash_selected(&mut self, session: &mut FileManagerSession) -> bool {
        let mut changed = false;
        for id in &session.selected {
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.id == *id && !entry.trashed)
            {
                entry.trashed = true;
                changed = true;
            }
        }
        if changed {
            session.filter = EntryFilter::Trash;
            session.notice = "已移至回收站".to_owned();
        }
        changed
    }

    fn restore_selected(&mut self, session: &mut FileManagerSession) -> bool {
        let mut changed = false;
        for id in &session.selected {
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.id == *id && entry.trashed)
            {
                entry.trashed = false;
                changed = true;
            }
        }
        if changed {
            session.filter = EntryFilter::All;
            session.notice = "已恢复选中项目".to_owned();
        }
        changed
    }

    fn toggle_favorite(&mut self, session: &mut FileManagerSession) -> bool {
        let mut changed = false;
        for id in &session.selected {
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.id == *id && !entry.trashed)
            {
                entry.favorite = !entry.favorite;
                changed = true;
            }
        }
        if changed {
            session.notice = "已更新收藏状态".to_owned();
        }
        changed
    }

    fn add_tag_named(&mut self, session: &mut FileManagerSession, tag: String) -> bool {
        let tag = tag.trim();
        if tag.is_empty() {
            return false;
        }
        let mut changed = false;
        for id in &session.selected {
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.id == *id && !entry.trashed)
                && !entry.tags.iter().any(|current| current == tag)
            {
                entry.tags.push(tag.to_owned());
                changed = true;
            }
        }
        if changed {
            session.notice = format!("已添加 {tag} 标签");
        }
        changed
    }
}

impl Default for FileManagerSession {
    fn default() -> Self {
        Self {
            current_dir: 1,
            selected: BTreeSet::new(),
            query: String::new(),
            view: DirectoryView::List,
            show_navigation: false,
            sort: SortMode::Name,
            filter: EntryFilter::All,
            operation: None,
            notice: "工作区已准备就绪".to_owned(),
        }
    }
}

impl FileManagerSession {
    pub fn select(&mut self, id: EntryId) {
        self.selected = BTreeSet::from([id]);
    }
}

fn folder(id: EntryId, parent: Option<EntryId>, name: &str, modified: &'static str) -> Entry {
    Entry {
        id,
        parent,
        name: name.to_owned(),
        kind: EntryKind::Folder,
        bytes: 0,
        modified,
        favorite: false,
        tags: Vec::new(),
        trashed: false,
        text: None,
    }
}

fn file(
    id: EntryId,
    parent: Option<EntryId>,
    name: &str,
    kind: EntryKind,
    bytes: u64,
    modified: &'static str,
    text: Option<&'static str>,
) -> Entry {
    Entry {
        id,
        parent,
        name: name.to_owned(),
        kind,
        bytes,
        modified,
        favorite: id == 5,
        tags: vec!["demo".to_owned()],
        trashed: false,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_change_only_the_memory_workspace_and_undo_restores_it() {
        let mut model = FileManagerModel::sample();
        let mut session = FileManagerSession::default();
        session.select(5);
        model.apply(
            &mut session,
            FileCommand::RenameSelectedAs("已重命名-README.md".to_owned()),
        );
        assert!(model.entry(5).unwrap().name.starts_with("已重命名-"));
        model.apply(&mut session, FileCommand::Undo);
        assert_eq!(model.entry(5).unwrap().name, "README.md");
    }

    #[test]
    fn copied_entries_get_new_business_ids_without_exposing_tela_keys() {
        let mut model = FileManagerModel::sample();
        let mut session = FileManagerSession::default();
        session.select(5);
        model.apply(&mut session, FileCommand::CopySelected);
        let copied = *session.selected.iter().next().unwrap();
        assert_ne!(copied, 5);
        assert!(model.entry(copied).unwrap().name.ends_with("副本"));
    }

    #[test]
    fn special_filters_search_the_workspace_while_all_stays_in_the_current_directory() {
        let model = FileManagerModel::sample();
        assert_eq!(
            model
                .entries_in_filtered(2, "", EntryFilter::All, SortMode::Name)
                .len(),
            2
        );
        assert!(
            model
                .entries_in_filtered(2, "README", EntryFilter::Favorites, SortMode::Name)
                .iter()
                .any(|entry| entry.id == 5)
        );
    }
}
