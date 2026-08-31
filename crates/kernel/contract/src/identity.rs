//! 身份维度：跨帧 key、身份策略与更新模式（见 005-key身份策略、004-更新策略与状态保持）。

/// 跨帧稳定的 key，用于节点匹配、状态复用与视图状态仓库索引（见 003-场景树与节点模型 4）。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticKey(pub String);

impl From<String> for SemanticKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SemanticKey {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// DSL assemble 使用的父范围局部 key 片段。
///
/// 此类型只连接 Composition DSL 与 Kernel 的身份解析。应用代码应使用 `<For
/// key={...}>`，而不是直接构造它；因此它不属于稳定的业务 API。
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeySegment {
    value: String,
    collection_scope: Option<u64>,
}

impl KeySegment {
    /// 从已经计算出的局部业务标识创建片段。
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            collection_scope: None,
        }
    }

    /// 返回未编码的局部业务标识。
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// 为透明 DSL collection 设置内部命名空间。
    ///
    /// 这不是业务 API。Composition lowering 必须为每一个 `<For>` 调用它，即使当前父节点
    /// 下只有一个 collection；Kernel 将 scope 与局部业务标识一起合成为最终 `SemanticKey`。
    #[doc(hidden)]
    pub fn with_collection_scope(mut self, scope: u64) -> Self {
        self.collection_scope = Some(scope);
        self
    }

    /// 返回 DSL lowering 指定的内部 collection namespace。
    #[doc(hidden)]
    pub fn collection_scope(&self) -> Option<u64> {
        self.collection_scope
    }
}

impl From<String> for KeySegment {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for KeySegment {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// key 身份策略（见 005-key身份策略）。
///
/// 配置在容器节点的 `IdentityConcern` 上，描述该节点自己的 key 来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStrategy {
    /// 按节点树位置路径自动生成 key。全局默认。
    AutoPath,
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

/// `IdentityConcern` 槽位：key 策略 / 更新模式 / 语义 id（见 003-场景树与节点模型 1.1）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityConcern {
    /// key 身份策略，默认 `AutoPath`。
    pub key_strategy: KeyStrategy,
    /// 更新模式，默认 `Full`。
    pub update_mode: UpdateMode,
    /// 业务显式语义 id（`SemanticId`/`Manual` 策略下必须提供）。
    pub semantic_key: Option<SemanticKey>,
    /// DSL `For` 的父范围局部业务 key。
    ///
    /// Kernel 在 identity 解析阶段将它与已经解析的父 `SemanticKey` 合成最终全树 key。
    /// 它不能与 `semantic_key` 同时声明，且根节点不能只使用此字段。
    #[doc(hidden)]
    pub key_segment: Option<KeySegment>,
}

impl Default for IdentityConcern {
    fn default() -> Self {
        Self {
            key_strategy: KeyStrategy::AutoPath,
            update_mode: UpdateMode::Full,
            semantic_key: None,
            key_segment: None,
        }
    }
}
