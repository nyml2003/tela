//! 身份维度：跨帧 key、身份策略与更新模式（见 005-key身份策略、004-更新策略与状态保持）。

/// 跨帧稳定的 key，用于节点匹配、状态复用与视图状态仓库索引（见 003-场景树与节点模型 4）。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticKey(pub String);

/// key 身份策略（见 005-key身份策略）。
///
/// 配置在容器节点的 `IdentityConcern` 上，向下生效，子容器可覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStrategy {
    /// 按节点树位置路径自动生成 key。全局默认。
    AutoPath,
    /// 首次出现自动分配内部稳定身份，增删/重排保持。
    AutoStableIdentity,
    /// 复用业务已有实体主键。
    SemanticId,
    /// 业务完全自行提供 key，高级兜底。
    Manual,
}

/// 子树更新模式（见 004-更新策略与状态保持）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateMode {
    /// 整体重建：整棵子树完整重新布局与绘制。
    Full,
    /// 局部 Dirty：仅重算被标记变更的局部子树。
    Dirty,
}

/// `IdentityConcern` 槽位：key 策略 / 更新模式 / 语义 id（向下生效，见 003-场景树与节点模型 1.1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityConcern {
    /// key 身份策略，默认 `AutoPath`。
    pub key_strategy: KeyStrategy,
    /// 更新模式，默认 `Full`。
    pub update_mode: UpdateMode,
    /// 业务显式语义 id（`SemanticId`/`Manual` 策略下必须提供）。
    pub semantic_key: Option<SemanticKey>,
}

impl Default for IdentityConcern {
    fn default() -> Self {
        Self {
            key_strategy: KeyStrategy::AutoPath,
            update_mode: UpdateMode::Full,
            semantic_key: None,
        }
    }
}
