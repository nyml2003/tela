//! Transfer：双栏数据选择组件。
//!
//! 最终目标 key 集合由上层受控；搜索词、左右临时勾选和移动中间态由组件运行时持有。
//! 该模块的状态机不依赖窗口、异步任务或业务进程模型，可在桌面和 Headless 测试中复用。

use std::collections::BTreeSet;

use tela_contract::{
    Color, Fill, IdentityConcern, InteractConcern, KeyStrategy, LayoutConcern, SemanticKey, Size,
    UiNode, UpdateMode, VisualConcern,
};
use tela_core::LayoutContainer;
use tela_ui_foundation::Input;

use crate::shared::{BORDER, FIELD_BG, TEXT, TEXT_SECONDARY, text};

/// Transfer 的一项数据。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferItem {
    /// 业务唯一 key。
    pub key: String,
    /// 显示文本。
    pub label: String,
    /// 禁止被移动或勾选。
    pub disabled: bool,
}

impl TransferItem {
    /// 创建可选项。
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            disabled: false,
        }
    }

    /// 设置禁用态。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Transfer 的局部交互事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferEvent {
    /// 设置左侧搜索词。
    LeftSearch(String),
    /// 设置右侧搜索词。
    RightSearch(String),
    /// 切换左侧临时勾选。
    ToggleLeft(String),
    /// 切换右侧临时勾选。
    ToggleRight(String),
    /// 将左侧勾选项移入右侧。
    MoveRight,
    /// 将右侧勾选项移回左侧。
    MoveLeft,
    /// 清空左右临时勾选。
    ClearChecks,
}

/// Transfer 事件的上层输出。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferOutcome {
    /// 是否需要重绘组件。
    pub changed: bool,
    /// 若发生移动，返回新的最终目标 key 集合。
    pub target_keys: Option<BTreeSet<String>>,
}

/// Transfer 的跨帧私有状态。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransferState {
    left_search: String,
    right_search: String,
    left_checked: BTreeSet<String>,
    right_checked: BTreeSet<String>,
}

impl TransferState {
    /// 创建空的局部状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 左侧搜索词。
    pub fn left_search(&self) -> &str {
        &self.left_search
    }

    /// 右侧搜索词。
    pub fn right_search(&self) -> &str {
        &self.right_search
    }

    /// 左侧临时勾选。
    pub fn left_checked(&self) -> &BTreeSet<String> {
        &self.left_checked
    }

    /// 右侧临时勾选。
    pub fn right_checked(&self) -> &BTreeSet<String> {
        &self.right_checked
    }

    /// 返回受控文本输入通道需要的当前值；未知绑定不会暴露组件内部字段。
    pub fn input_value(&self, bind_id: &str) -> Option<&str> {
        match bind_id {
            "transfer.left-search" => Some(&self.left_search),
            "transfer.right-search" => Some(&self.right_search),
            _ => None,
        }
    }

    /// 处理局部事件；`target_keys` 是唯一会泄漏给应用的业务输出。
    pub fn handle(
        &mut self,
        event: TransferEvent,
        items: &[TransferItem],
        target_keys: &BTreeSet<String>,
    ) -> TransferOutcome {
        let before = self.clone();
        let mut output = None;
        match event {
            TransferEvent::LeftSearch(value) => self.left_search = value,
            TransferEvent::RightSearch(value) => self.right_search = value,
            TransferEvent::ToggleLeft(key) => toggle_checked(&mut self.left_checked, key),
            TransferEvent::ToggleRight(key) => toggle_checked(&mut self.right_checked, key),
            TransferEvent::MoveRight => {
                let mut next = target_keys.clone();
                for key in self.left_checked.iter() {
                    if items
                        .iter()
                        .find(|item| item.key == *key)
                        .is_some_and(|item| !item.disabled)
                    {
                        next.insert(key.clone());
                    }
                }
                self.left_checked.clear();
                output = Some(next);
            }
            TransferEvent::MoveLeft => {
                let mut next = target_keys.clone();
                for key in self.right_checked.iter() {
                    next.remove(key);
                }
                self.right_checked.clear();
                output = Some(next);
            }
            TransferEvent::ClearChecks => {
                self.left_checked.clear();
                self.right_checked.clear();
            }
        }
        TransferOutcome {
            changed: before != *self || output.is_some(),
            target_keys: output,
        }
    }

    /// 当受控目标集合刷新后，移除已不存在的临时勾选。
    pub fn reconcile(&mut self, items: &[TransferItem], target_keys: &BTreeSet<String>) {
        let known = items
            .iter()
            .map(|item| item.key.as_str())
            .collect::<BTreeSet<_>>();
        self.left_checked
            .retain(|key| known.contains(key.as_str()) && !target_keys.contains(key));
        self.right_checked
            .retain(|key| known.contains(key.as_str()) && target_keys.contains(key));
    }
}

fn toggle_checked(checked: &mut BTreeSet<String>, key: String) {
    if !checked.insert(key.clone()) {
        checked.remove(&key);
    }
}

/// Transfer 的受控外观配置。
pub struct Transfer {
    items: Vec<TransferItem>,
    target_keys: BTreeSet<String>,
    state: TransferState,
    width: f32,
    height: f32,
    key: Option<SemanticKey>,
}

impl Transfer {
    /// 创建 Transfer；`target_keys` 是应用拥有的最终值，`state` 是组件私有状态快照。
    pub fn new(
        items: Vec<TransferItem>,
        target_keys: impl IntoIterator<Item = String>,
        state: TransferState,
    ) -> Self {
        Self {
            items,
            target_keys: target_keys.into_iter().collect(),
            state,
            width: 520.0,
            height: 260.0,
            key: None,
        }
    }

    /// 固定整体宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(260.0);
        self
    }

    /// 固定整体高度。
    pub fn height(mut self, height: f32) -> Self {
        self.height = height.max(160.0);
        self
    }

    /// 设置稳定语义 key。
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(SemanticKey(key.into()));
        self
    }

    /// 读取当前私有状态快照。
    pub fn state(&self) -> &TransferState {
        &self.state
    }

    /// 读取受控目标 key 集合。
    pub fn target_keys(&self) -> &BTreeSet<String> {
        &self.target_keys
    }

    /// 生成双栏节点树。
    pub fn into_node(self) -> UiNode {
        let left = self
            .items
            .iter()
            .filter(|item| !self.target_keys.contains(&item.key));
        let right = self
            .items
            .iter()
            .filter(|item| self.target_keys.contains(&item.key));
        let left = panel(
            "可选项",
            self.state.left_search(),
            left.filter(|item| matches_query(item, self.state.left_search()))
                .map(|item| row(item, self.state.left_checked.contains(&item.key)))
                .collect(),
            self.width / 2.0 - 34.0,
            self.height,
            "transfer.left-search",
        );
        let right = panel(
            "目标项",
            self.state.right_search(),
            right
                .filter(|item| matches_query(item, self.state.right_search()))
                .map(|item| row(item, self.state.right_checked.contains(&item.key)))
                .collect(),
            self.width / 2.0 - 34.0,
            self.height,
            "transfer.right-search",
        );
        let actions: UiNode = LayoutContainer::column([
            action_button(
                "→",
                !self.state.left_checked.is_empty(),
                "transfer.move-right",
            ),
            action_button(
                "←",
                !self.state.right_checked.is_empty(),
                "transfer.move-left",
            ),
        ])
        .layout(LayoutConcern {
            width: Some(Size::fixed(44.0)),
            gap: 8.0,
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into();
        let mut node: UiNode = LayoutContainer::row([left, actions, right])
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(self.height)),
                gap: 10.0,
                ..LayoutConcern::default()
            })
            .into();
        if let Some(key) = self.key {
            node.identity = Some(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(key),
                update_mode: UpdateMode::Dirty,
                ..IdentityConcern::default()
            });
        }
        node
    }
}

fn matches_query(item: &TransferItem, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || item.label.to_lowercase().contains(&query)
        || item.key.to_lowercase().contains(&query)
}

fn panel(
    title: &str,
    query: &str,
    rows: Vec<UiNode>,
    width: f32,
    height: f32,
    bind_id: &str,
) -> UiNode {
    LayoutContainer::column([
        text(title, 12.0, TEXT_SECONDARY),
        Input::new()
            .value(query)
            .placeholder("搜索")
            .bind_id(bind_id)
            .into_node(),
        LayoutContainer::scroll_view(rows)
            .visual(VisualConcern {
                fill: Some(Fill::Solid(FIELD_BG)),
                border_color: Some(BORDER),
                border_radius: tela_contract::BorderRadius::all(4.0),
                ..VisualConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::fixed(width)),
                height: Some(Size::fixed(height - 48.0)),
                padding: tela_contract::Insets::all(6.0),
                ..LayoutConcern::default()
            })
            .into(),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(width)),
        height: Some(Size::fixed(height)),
        gap: 6.0,
        ..LayoutConcern::default()
    })
    .into()
}

fn row(item: &TransferItem, checked: bool) -> UiNode {
    let mut node: UiNode = LayoutContainer::row([
        text(
            if checked { "☑" } else { "□" },
            13.0,
            if item.disabled { TEXT_SECONDARY } else { TEXT },
        ),
        text(
            &item.label,
            13.0,
            if item.disabled { TEXT_SECONDARY } else { TEXT },
        ),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(220.0)),
        height: Some(Size::fixed(26.0)),
        gap: 6.0,
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .into();
    node.identity = Some(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(format!("transfer.item.{}", item.key))),
        update_mode: UpdateMode::Dirty,
        ..IdentityConcern::default()
    });
    if !item.disabled {
        node.interact = Some(InteractConcern {
            clickable: true,
            hoverable: true,
            focusable: true,
            ..InteractConcern::default()
        });
    }
    node
}

fn action_button(label: &str, enabled: bool, key: &str) -> UiNode {
    let mut node: UiNode = LayoutContainer::row([text(
        label,
        16.0,
        if enabled { TEXT } else { TEXT_SECONDARY },
    )])
    .visual(VisualConcern {
        fill: Some(Fill::Solid(if enabled {
            Color::rgba(0.93, 0.95, 0.99, 1.0)
        } else {
            Color::rgba(0.96, 0.96, 0.97, 1.0)
        })),
        border_color: Some(BORDER),
        border_radius: tela_contract::BorderRadius::all(4.0),
        ..VisualConcern::default()
    })
    .layout(LayoutConcern {
        width: Some(Size::fixed(36.0)),
        height: Some(Size::fixed(30.0)),
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .into();
    node.identity = Some(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key.to_owned())),
        update_mode: UpdateMode::Dirty,
        ..IdentityConcern::default()
    });
    if enabled {
        node.interact = Some(InteractConcern {
            clickable: true,
            hoverable: true,
            focusable: true,
            ..InteractConcern::default()
        });
    }
    node
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{TransferEvent, TransferItem, TransferState};

    fn items() -> Vec<TransferItem> {
        vec![
            TransferItem::new("a", "Alpha"),
            TransferItem::new("b", "Beta"),
        ]
    }

    #[test]
    fn search_and_checks_stay_local_until_move() {
        let items = items();
        let mut state = TransferState::new();
        let target = BTreeSet::new();
        state.handle(TransferEvent::LeftSearch("alp".to_owned()), &items, &target);
        state.handle(TransferEvent::ToggleLeft("a".to_owned()), &items, &target);
        assert_eq!(state.left_search(), "alp");
        let outcome = state.handle(TransferEvent::MoveRight, &items, &target);
        assert_eq!(outcome.target_keys, Some(BTreeSet::from(["a".to_owned()])));
    }

    #[test]
    fn disabled_items_cannot_move() {
        let items = vec![TransferItem::new("a", "Alpha").disabled(true)];
        let mut state = TransferState::new();
        let target = BTreeSet::new();
        state.handle(TransferEvent::ToggleLeft("a".to_owned()), &items, &target);
        assert_eq!(
            state
                .handle(TransferEvent::MoveRight, &items, &target)
                .target_keys,
            Some(BTreeSet::new())
        );
    }
}
