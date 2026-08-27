//! 应用层：把组件交互意图翻译为领域命令。

use crate::domain::{
    EntryFilter, EntryId, FileCommand, FileManagerModel, FileManagerSession, OperationKind,
};

pub mod controller;
#[cfg(test)]
mod tests;

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
