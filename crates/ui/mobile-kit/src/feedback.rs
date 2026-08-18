//! 移动页面的空内容反馈。

use tela_contract::{
    BorderRadius, Color, Fill, IdentityConcern, InteractConcern, KeyStrategy, LayoutConcern,
    SemanticKey, Size, UiNode, UpdateMode, VisualConcern,
};
use tela_core::LayoutContainer;

use crate::MIN_TOUCH_TARGET;
use crate::shared::{DISABLED_TEXT, PRIMARY, SUBTLE_SURFACE, TEXT, TEXT_SECONDARY, text};

/// 空态中由 Application 处理的可选恢复动作。
pub struct MobileEmptyAction {
    label: String,
    action_key: SemanticKey,
    disabled: bool,
}

impl MobileEmptyAction {
    /// 创建恢复动作；`action_key` 是稳定动作键。
    pub fn new(label: impl Into<String>, action_key: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action_key: SemanticKey(action_key.into()),
            disabled: false,
        }
    }

    /// 投影禁用态。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// 移动空态的可替换视觉参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MobileEmptyStateStyle {
    /// 内容区域的背景。默认透明，避免把整页空态做成装饰卡片。
    pub surface: Color,
    /// 内容区域圆角。
    pub border_radius: BorderRadius,
    /// 标题颜色。
    pub title: Color,
    /// 说明颜色。
    pub description: Color,
    /// 恢复动作背景色。
    pub action_fill: Color,
    /// 恢复动作文字色。
    pub action_text: Color,
    /// 禁用恢复动作背景色。
    pub disabled_action_fill: Color,
    /// 禁用恢复动作文字色。
    pub disabled_action_text: Color,
}

impl Default for MobileEmptyStateStyle {
    fn default() -> Self {
        Self {
            surface: Color::TRANSPARENT,
            border_radius: BorderRadius::all(8.0),
            title: TEXT,
            description: TEXT_SECONDARY,
            action_fill: PRIMARY,
            action_text: Color::WHITE,
            disabled_action_fill: SUBTLE_SURFACE,
            disabled_action_text: DISABLED_TEXT,
        }
    }
}

/// 无数据、筛选无结果或首次使用时的移动空态。
pub struct MobileEmptyState {
    title: String,
    description: Option<String>,
    action: Option<MobileEmptyAction>,
    width: f32,
    height: f32,
    style: MobileEmptyStateStyle,
}

impl MobileEmptyState {
    /// 创建带可读标题的空态。
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            action: None,
            width: 280.0,
            height: 160.0,
            style: MobileEmptyStateStyle::default(),
        }
    }

    /// 添加帮助用户恢复上下文的说明。
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// 添加一个可选恢复动作。
    pub fn action(mut self, action: MobileEmptyAction) -> Self {
        self.action = Some(action);
        self
    }

    /// 设置保留空间，避免数据载入前后发生布局跳动。
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        self
    }

    /// 覆盖空态视觉参数。
    pub fn style(mut self, style: MobileEmptyStateStyle) -> Self {
        self.style = style;
        self
    }

    /// 构建居中的标题、说明和可选恢复动作。
    pub fn into_node(self) -> UiNode {
        let style = self.style;
        let mut content = vec![text(&self.title, 16.0, style.title)];
        if let Some(description) = self.description {
            content.push(text(&description, 14.0, style.description));
        }
        if let Some(action) = self.action {
            content.push(action_node(action, style));
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
        .visual(VisualConcern {
            fill: Some(Fill::Solid(style.surface)),
            border_radius: style.border_radius,
            ..VisualConcern::default()
        })
        .into()
    }
}

impl From<MobileEmptyState> for UiNode {
    fn from(empty: MobileEmptyState) -> Self {
        empty.into_node()
    }
}

fn action_node(action: MobileEmptyAction, style: MobileEmptyStateStyle) -> UiNode {
    let (fill, text_color) = if action.disabled {
        (style.disabled_action_fill, style.disabled_action_text)
    } else {
        (style.action_fill, style.action_text)
    };
    let mut node: UiNode = LayoutContainer::row([
        LayoutContainer::spacer().into(),
        text(&action.label, 14.0, text_color),
        LayoutContainer::spacer().into(),
    ])
    .layout(LayoutConcern {
        width: Some(Size::fixed(128.0)),
        height: Some(Size::fixed(MIN_TOUCH_TARGET)),
        cross_align: tela_contract::CrossAlign::Center,
        ..LayoutConcern::default()
    })
    .visual(VisualConcern {
        fill: Some(Fill::Solid(fill)),
        border_radius: BorderRadius::all(8.0),
        ..VisualConcern::default()
    })
    .identity(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(action.action_key),
        key_segment: None,
        update_mode: UpdateMode::Dirty,
    })
    .into();
    if !action.disabled {
        node.interact = Some(InteractConcern {
            clickable: true,
            focusable: true,
            ..InteractConcern::default()
        });
    }
    node
}

#[cfg(test)]
mod tests {
    use super::{MobileEmptyAction, MobileEmptyState};
    use crate::MIN_TOUCH_TARGET;
    use tela_contract::Size;

    #[test]
    fn empty_state_reserves_space_and_exposes_its_recovery_target() {
        let node = MobileEmptyState::new("没有匹配的内容")
            .description("修改筛选条件后重试")
            .action(MobileEmptyAction::new("清除筛选", "mobile.clear-filter"))
            .size(320.0, 200.0)
            .into_node();

        assert_eq!(
            node.layout.as_ref().and_then(|layout| layout.width),
            Some(Size::fixed(320.0))
        );
        let action = node.children[1].children.last().expect("action");
        assert_eq!(
            action.layout.as_ref().and_then(|layout| layout.height),
            Some(Size::fixed(MIN_TOUCH_TARGET))
        );
        assert_eq!(
            action
                .identity
                .as_ref()
                .and_then(|identity| identity.semantic_key.as_ref())
                .map(|id| id.0.as_str()),
            Some("mobile.clear-filter")
        );
    }
}
