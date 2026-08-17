//! 业务领域：文件实体、工作区状态与可撤销命令。

mod model;

pub use model::{
    DirectoryView, Entry, EntryFilter, EntryId, EntryKind, FileCommand, FileManagerModel,
    FileManagerSession, OperationDraft, OperationKind, SortMode,
};
