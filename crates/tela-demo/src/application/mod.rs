//! 应用层：把组件交互意图翻译为领域命令。

use crate::domain::{
    EntryFilter, EntryId, FileCommand, FileManagerModel, FileManagerSession, OperationKind,
};

pub mod reactive;
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
    ConfirmOperation,
    CancelOperation,
}

/// 由组件声明的业务绑定路由意图。绑定值不是节点 key；普通组件不需要管理 tela identity。
pub fn intent_from_bind_id(bind_id: &str) -> Option<Intent> {
    let command = match bind_id {
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
    if bind_id == "navigation.toggle" {
        return Some(Intent::ToggleNavigation);
    }
    let filter = match bind_id {
        "filter.all" => Some(EntryFilter::All),
        "filter.favorites" => Some(EntryFilter::Favorites),
        "filter.tagged" => Some(EntryFilter::Tagged),
        "filter.trash" => Some(EntryFilter::Trash),
        _ => None,
    };
    if let Some(filter) = filter {
        return Some(Intent::SetFilter(filter));
    }
    if bind_id == "operation.confirm" {
        return Some(Intent::ConfirmOperation);
    }
    if bind_id == "operation.cancel" {
        return Some(Intent::CancelOperation);
    }
    if let Some(id) = bind_id.strip_prefix("folder.open.") {
        return id.parse().ok().map(Intent::OpenFolder);
    }
    bind_id
        .strip_prefix("entry.select.")?
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
