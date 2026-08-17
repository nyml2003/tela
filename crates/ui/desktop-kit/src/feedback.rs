//! 桌面工作台的状态与空内容反馈组件。

use tela_contract::{
    BorderRadius, Color, Fill, IdentityConcern, KeyStrategy, LayoutConcern, SemanticKey, Size,
    UiNode, UpdateMode, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};
use tela_ui_foundation::{Button, ButtonVariant};

use crate::shared::{TEXT, TEXT_SECONDARY, text};

/// 状态反馈的语义颜色，不以颜色单独表达状态：始终与文字一起输出。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusTone {
    /// 中性状态。
    #[default]
    Default,
    /// 成功或已完成。
    Success,
    /// 处理中。
    Processing,
    /// 需要注意。
    Warning,
    /// 失败或阻塞。
    Error,
}

impl StatusTone {
    const fn color(self) -> Color {
        match self {
            Self::Default => Color::rgba(0.49, 0.53, 0.60, 1.0),
            Self::Success => Color::rgba(0.20, 0.60, 0.36, 1.0),
            Self::Processing => Color::rgba(0.15, 0.39, 0.92, 1.0),
            Self::Warning => Color::rgba(0.83, 0.48, 0.07, 1.0),
            Self::Error => Color::rgba(0.80, 0.19, 0.22, 1.0),
        }
    }
}

/// 带文字说明的状态点，适合列表、表格与详情摘要。
pub struct StatusBadge {
    label: String,
    tone: StatusTone,
    dot_size: f32,
}

impl StatusBadge {
    /// 创建状态点和文字，不会只以颜色表达状态。
    pub fn new(label: impl Into<String>, tone: StatusTone) -> Self {
        Self {
            label: label.into(),
            tone,
            dot_size: 8.0,
        }
    }

    /// 设置状态点尺寸。
    pub fn dot_size(mut self, size: f32) -> Self {
        self.dot_size = size.max(4.0);
        self
    }

    /// 构建状态点和文本。
    pub fn into_node(self) -> UiNode {
        let dot: UiNode = Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.dot_size)),
                height: Some(Size::fixed(self.dot_size)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.tone.color())),
                border_radius: BorderRadius::all(self.dot_size * 0.5),
                ..VisualConcern::default()
            })
            .into();
        LayoutContainer::row([dot, text(&self.label, 12.0, TEXT_SECONDARY)])
            .layout(LayoutConcern {
                gap: 6.0,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .into()
    }
}

impl From<StatusBadge> for UiNode {
    fn from(badge: StatusBadge) -> Self {
        badge.into_node()
    }
}

/// 空态中的可选主动作。
pub struct EmptyAction {
    label: String,
    action_key: SemanticKey,
    disabled: bool,
}

impl EmptyAction {
    /// 创建动作。`action_key` 由 Application 路由。
    pub fn new(label: impl Into<String>, action_key: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action_key: SemanticKey(action_key.into()),
            disabled: false,
        }
    }

    /// 禁用动作。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// 无数据、首次使用或筛选无结果时的桌面空态。
pub struct EmptyState {
    title: String,
    description: Option<String>,
    action: Option<EmptyAction>,
    width: f32,
    height: f32,
}

impl EmptyState {
    /// 创建空态标题。
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            action: None,
            width: 320.0,
            height: 180.0,
        }
    }

    /// 添加可读的恢复或下一步说明。
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 添加一个可选主动作。
    pub fn action(mut self, action: EmptyAction) -> Self {
        self.action = Some(action);
        self
    }

    /// 设置保留空间，异步数据出现前后不会产生布局跳动。
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        self
    }

    /// 构建居中的标题、说明和可选恢复动作。
    pub fn into_node(self) -> UiNode {
        let mut content = vec![text(&self.title, 15.0, TEXT)];
        if let Some(description) = self.description {
            content.push(text(&description, 13.0, TEXT_SECONDARY));
        }
        if let Some(action) = self.action {
            let mut button = Button::new(action.label)
                .variant(ButtonVariant::Primary)
                .width(92.0)
                .height(30.0)
                .border_radius(6.0)
                .disabled(action.disabled)
                .into_node();
            button.identity = Some(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(action.action_key),
                update_mode: UpdateMode::Dirty,
            });
            content.push(button);
        }
        let content: UiNode = LayoutContainer::column(content)
            .layout(LayoutConcern {
                gap: 10.0,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .into();
        LayoutContainer::row([
            LayoutContainer::spacer().into(),
            content,
            LayoutContainer::spacer().into(),
        ])
        .layout(LayoutConcern {
            width: Some(Size::fixed(self.width)),
            height: Some(Size::fixed(self.height)),
            cross_align: tela_contract::CrossAlign::Center,
            ..LayoutConcern::default()
        })
        .into()
    }
}

impl From<EmptyState> for UiNode {
    fn from(empty: EmptyState) -> Self {
        empty.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{EmptyAction, EmptyState, StatusBadge, StatusTone};
    use tela_contract::{ContentConcern, Fill};

    #[test]
    fn status_badge_pairs_visible_text_with_the_status_dot() {
        let node = StatusBadge::new("同步失败", StatusTone::Error).into_node();
        assert!(matches!(
            node.children[0]
                .visual
                .as_ref()
                .and_then(|visual| visual.fill.as_ref()),
            Some(Fill::Solid(_))
        ));
        assert!(matches!(
            node.children[1].content,
            Some(ContentConcern::Text(ref text)) if text.text == "同步失败"
        ));
    }

    #[test]
    fn empty_state_reserves_space_and_keeps_its_recovery_action_named() {
        let node = EmptyState::new("没有匹配结果")
            .description("修改筛选条件后重试")
            .action(EmptyAction::new("清除筛选", "files.clear-filter"))
            .size(400.0, 200.0)
            .into_node();
        assert_eq!(
            node.layout.as_ref().and_then(|layout| layout.width),
            Some(tela_contract::Size::fixed(400.0))
        );
        let content = &node.children[1];
        let action = content.children.last().expect("action");
        assert_eq!(
            action
                .identity
                .as_ref()
                .and_then(|identity| identity.semantic_key.as_ref())
                .map(|target| target.0.as_str()),
            Some("files.clear-filter")
        );
    }
}
