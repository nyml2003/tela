//! 应用层：把组件交互意图翻译为领域命令。

use crate::domain::{
    EntryFilter, EntryId, FileCommand, FileManagerModel, FileManagerSession, OperationKind,
};
use tela_ui_headless::ComponentPartPath;

pub mod keymap;
pub mod runtime;

/// View 可发出的语义意图；不暴露 tela `NodeId` 或 `SemanticKey`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Command(FileCommand),
    Select(EntryId),
    OpenFolder(EntryId),
    SetFilter(EntryFilter),
    ToggleNavigation,
    BeginOperation(OperationKind),
    SetOperationValue(String),
    SetQuery(String),
    ConfirmOperation,
    CancelOperation,
}

/// 将一个显式组件分部路径翻译为业务意图。
///
/// 路径不是字段绑定，也不是临时 `NodeId`。Application 在接到 headless `Activate` 后才调用
/// 本函数，因此不会回退为不透明的字符串命令编解码。
pub fn intent_from_component_part(part: &ComponentPartPath) -> Option<Intent> {
    let path = part.item_key().unwrap_or_else(|| part.as_str());
    let command = match path {
        "command.new-folder" => return Some(Intent::BeginOperation(OperationKind::NewFolder)),
        "command.rename" => return Some(Intent::BeginOperation(OperationKind::Rename)),
        "command.copy" => Some(FileCommand::CopySelected),
        "command.move-design" => return Some(Intent::BeginOperation(OperationKind::MoveToDesign)),
        "command.trash" => return Some(Intent::BeginOperation(OperationKind::Trash)),
        "command.restore" => Some(FileCommand::RestoreSelected),
        "command.favorite" => Some(FileCommand::ToggleFavorite),
        "command.toggle-view" => Some(FileCommand::ToggleView),
        "command.toggle-sort" => Some(FileCommand::ToggleSort),
        "command.toggle-filter" => Some(FileCommand::ToggleFilter),
        "command.add-tag" => return Some(Intent::BeginOperation(OperationKind::AddTag)),
        "command.undo" => Some(FileCommand::Undo),
        _ => None,
    };
    if let Some(command) = command {
        return Some(Intent::Command(command));
    }
    if path == "navigation.toggle" {
        return Some(Intent::ToggleNavigation);
    }
    let filter = match path {
        "filter.all" => Some(EntryFilter::All),
        "filter.favorites" => Some(EntryFilter::Favorites),
        "filter.tagged" => Some(EntryFilter::Tagged),
        "filter.trash" => Some(EntryFilter::Trash),
        _ => None,
    };
    if let Some(filter) = filter {
        return Some(Intent::SetFilter(filter));
    }
    if path == "operation.confirm" {
        return Some(Intent::ConfirmOperation);
    }
    if path == "operation.cancel" {
        return Some(Intent::CancelOperation);
    }
    if let Some(id) = path.strip_prefix("folder.open.") {
        return id.parse().ok().map(Intent::OpenFolder);
    }
    path.strip_prefix("entry.select.")
        .or_else(|| path.strip_prefix("entry-"))?
        .parse()
        .ok()
        .map(Intent::Select)
}

/// 唯一业务写入口。
pub fn apply_intent(
    model: &mut FileManagerModel,
    session: &mut FileManagerSession,
    intent: Intent,
) {
    match intent {
        Intent::Command(command) => model.apply(session, command),
        Intent::Select(id) => {
            session.select(id);
            session.notice = format!(
                "已选择 {}",
                model
                    .entry(id)
                    .map(|entry| entry.name.as_str())
                    .unwrap_or("项目")
            );
        }
        Intent::OpenFolder(id) => {
            session.current_dir = id;
            session.filter = EntryFilter::All;
            session.selected.clear();
            session.notice = "已切换目录".to_owned();
        }
        Intent::SetFilter(filter) => {
            session.filter = filter;
            session.selected.clear();
            session.notice = match filter {
                EntryFilter::All => "正在显示当前目录".to_owned(),
                EntryFilter::Favorites => "正在显示收藏项目".to_owned(),
                EntryFilter::Tagged => "正在显示已添加标签的项目".to_owned(),
                EntryFilter::Trash => "正在显示回收站".to_owned(),
            };
        }
        Intent::ToggleNavigation => session.show_navigation = !session.show_navigation,
        Intent::BeginOperation(kind) => {
            let value = match kind {
                OperationKind::NewFolder => "新建文件夹",
                OperationKind::Rename => model
                    .entry(
                        session
                            .selected
                            .iter()
                            .next()
                            .copied()
                            .unwrap_or(session.current_dir),
                    )
                    .map(|entry| entry.name.as_str())
                    .unwrap_or(""),
                OperationKind::MoveToDesign => "设计",
                OperationKind::AddTag => "重点",
                OperationKind::Trash => "",
            };
            session.operation = Some(crate::domain::OperationDraft {
                kind,
                value: value.to_owned(),
            });
        }
        Intent::SetOperationValue(value) => {
            if let Some(operation) = &mut session.operation {
                operation.value = value;
            }
        }
        Intent::SetQuery(value) => {
            session.query = value;
            session.notice = "已更新搜索结果".to_owned();
        }
        Intent::ConfirmOperation => {
            if let Some(draft) = session.operation.take() {
                let command = match draft.kind {
                    OperationKind::NewFolder => FileCommand::NewFolderNamed(draft.value),
                    OperationKind::Rename => FileCommand::RenameSelectedAs(draft.value),
                    OperationKind::MoveToDesign => FileCommand::MoveSelectedTo(2),
                    OperationKind::AddTag => FileCommand::AddTagNamed(draft.value),
                    OperationKind::Trash => FileCommand::TrashSelected,
                };
                model.apply(session, command);
            }
        }
        Intent::CancelOperation => {
            session.operation = None;
            session.notice = "已取消操作".to_owned();
        }
    }
}
