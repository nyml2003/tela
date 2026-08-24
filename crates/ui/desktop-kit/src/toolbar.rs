//! 主题无关的命令工具栏。

use tela_contract::{
    BorderRadius, Color, Fill, IconName, IconProvider, IdentityConcern, Insets, KeyStrategy,
    LayoutConcern, SemanticKey, Size, UiNode, UpdateMode, VisualConcern,
};
use tela_core::LayoutContainer;
use tela_ui_foundation::{Button, ButtonPalette, ButtonState, ButtonVariant};

use crate::IconButton;

/// Toolbar 的视觉与布局参数。
///
/// 调用方通过该类型提供主题值；Toolbar 不持有领域配色、图标资源或业务操作名。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolbarStyle {
    /// 工具栏背景。
    pub background: Color,
    /// 工具栏高度。
    pub height: f32,
    /// 项目之间的间距。
    pub gap: f32,
    /// 容器内边距。
    pub padding: Insets,
    /// 工具栏表面的圆角。
    pub border_radius: BorderRadius,
    /// 可选的工具栏表面边框颜色。
    pub border_color: Option<Color>,
    /// 工具栏表面边框宽度（逻辑像素）。
    pub border_width: f32,
    /// 工具栏内命令 Button 的圆角（逻辑像素）。
    pub button_border_radius: f32,
    /// 可选的原子 Button 调色板覆盖。
    pub button_palette: Option<ButtonPalette>,
    /// 可选的破坏性 Button 调色板覆盖。
    pub destructive_button_palette: Option<ButtonPalette>,
}

impl Default for ToolbarStyle {
    fn default() -> Self {
        Self {
            background: Color::WHITE,
            height: 40.0,
            gap: 6.0,
            padding: Insets {
                top: 0.0,
                right: 12.0,
                bottom: 0.0,
                left: 12.0,
            },
            border_radius: BorderRadius::default(),
            border_color: None,
            border_width: 0.0,
            button_border_radius: 6.0,
            button_palette: None,
            destructive_button_palette: None,
        }
    }
}

/// 工具栏中的一个可执行项。
#[derive(Clone, Debug, PartialEq)]
pub struct ToolbarItem {
    label: String,
    action_key: SemanticKey,
    icon: Option<IconName>,
    show_label: bool,
    disabled: bool,
    destructive: bool,
    hovered: bool,
    width: f32,
}

impl ToolbarItem {
    /// 创建一个普通命令项。
    pub fn new(label: impl Into<String>, action_key: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action_key: SemanticKey(action_key.into()),
            icon: None,
            show_label: true,
            disabled: false,
            destructive: false,
            hovered: false,
            width: 52.0,
        }
    }

    /// 设置该命令的语义图标。
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// 控制图标项是否显示文字标签，窄屏可仅保留图标。
    pub fn show_label(mut self, show_label: bool) -> Self {
        self.show_label = show_label;
        self
    }

    /// 设置禁用态。禁用项不会产生激活动作。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 标记为破坏性命令，交由原子 Button 选择对应的语义变体。
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    /// 设置来自 core view state 的悬停快照。
    ///
    /// 这是构建期输入，不在组件内持久化；调用方可在下一帧从 `ViewStateStore` 重新投影。
    pub fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// 设置固定逻辑宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// 返回此项的稳定组件动作 key。
    pub fn action_key(&self) -> &SemanticKey {
        &self.action_key
    }

    fn into_node(
        self,
        palette: Option<ButtonPalette>,
        destructive_palette: Option<ButtonPalette>,
        button_border_radius: f32,
        icons: &dyn IconProvider,
    ) -> UiNode {
        let action_key = self.action_key.clone();
        let mut button = if let Some(icon) = self.icon {
            let mut value = IconButton::new(icon)
                .size(self.width, 30.0)
                .border_radius(button_border_radius)
                .variant(if self.destructive {
                    ButtonVariant::Danger
                } else {
                    ButtonVariant::Primary
                })
                .state(ButtonState {
                    hovered: self.hovered,
                    selected: false,
                    disabled: self.disabled,
                });
            if self.show_label {
                value = value.label(self.label);
            }
            if let Some(palette) = if self.destructive {
                destructive_palette
            } else {
                palette
            } {
                value = value.palette(palette);
            }
            let mut node = value.into_node(icons);
            node.identity = Some(action_identity(action_key));
            return node;
        } else {
            // 兼容无图标的调用方，继续使用已有原子按钮。
            Button::new(self.label)
                .width(self.width)
                .height(26.0)
                .variant(if self.destructive {
                    ButtonVariant::Danger
                } else {
                    ButtonVariant::Primary
                })
                .disabled(self.disabled)
                .hovered(self.hovered)
                .border_radius(button_border_radius)
                .text_metrics(12.0, 15.0)
        };
        if let Some(palette) = if self.destructive {
            destructive_palette.or(palette)
        } else {
            palette
        } {
            button = button.palette(palette);
        }
        let mut node = button.into_node();
        node.identity = Some(action_identity(action_key));
        node
    }
}

/// 收纳在“更多”入口后的项目集合。
///
/// 本期不实现 Menu/Popover 的局部展开状态；宿主收到 [`ToolbarOverflow::target`] 的 `Invoke`
/// 后，可按 `items` 构建自己的菜单或在下一期接入 `tela-desktop-ui-kit::Menu`。
#[derive(Clone, Debug, PartialEq)]
pub struct ToolbarOverflow {
    label: String,
    action_key: SemanticKey,
    items: Vec<ToolbarItem>,
    width: f32,
}

impl ToolbarOverflow {
    /// 创建溢出入口及其逻辑项目。
    pub fn new(
        label: impl Into<String>,
        action_key: impl Into<String>,
        items: Vec<ToolbarItem>,
    ) -> Self {
        Self {
            label: label.into(),
            action_key: SemanticKey(action_key.into()),
            items,
            width: 52.0,
        }
    }

    /// 设置入口宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// 返回溢出入口的稳定组件动作 key。
    pub fn action_key(&self) -> &SemanticKey {
        &self.action_key
    }

    /// 返回由宿主在后续菜单中消费的项目。
    pub fn items(&self) -> &[ToolbarItem] {
        &self.items
    }

    fn trigger(&self) -> ToolbarItem {
        ToolbarItem::new(self.label.clone(), self.action_key.0.clone()).width(self.width)
    }
}

/// 一行命令项与可选溢出入口。
pub struct Toolbar {
    items: Vec<ToolbarItem>,
    overflow: Option<ToolbarOverflow>,
    prefix: Option<UiNode>,
    style: ToolbarStyle,
    hovered_action_key: Option<SemanticKey>,
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl Toolbar {
    /// 创建使用默认主题参数的空工具栏。
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            overflow: None,
            prefix: None,
            style: ToolbarStyle::default(),
            hovered_action_key: None,
        }
    }

    /// 追加一个可执行项。
    pub fn item(mut self, item: ToolbarItem) -> Self {
        self.items.push(item);
        self
    }

    /// 在命令项之前放置一个非交互前缀，例如当前路径。
    pub fn prefix(mut self, prefix: impl Into<UiNode>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// 以稳定组件动作 key 向指定项目投影 core 的当前悬停状态。
    pub fn hovered_action_key(mut self, action_key: Option<&SemanticKey>) -> Self {
        for item in &mut self.items {
            item.hovered = Some(item.action_key()) == action_key;
        }
        self.hovered_action_key = action_key.cloned();
        self
    }

    /// 设置由宿主处理打开逻辑的溢出入口。
    pub fn overflow(mut self, overflow: ToolbarOverflow) -> Self {
        self.overflow = Some(overflow);
        self
    }

    /// 覆盖主题与布局参数。
    pub fn style(mut self, style: ToolbarStyle) -> Self {
        self.style = style;
        self
    }

    /// 生成本帧节点树。
    ///
    /// Toolbar 的项目可由选择、权限或窗口宽度条件增删，因此它只在集合边界声明 core 的
    /// `AutoStableIdentity` 策略。每个命令的稳定语义 key 由 `ToolbarItem` 自身携带，
    /// FrameCoordinator 在当前帧把该 key 解析为组件事件路由。
    pub fn into_node(self, icons: &dyn IconProvider) -> UiNode {
        let mut children: Vec<UiNode> = self
            .items
            .into_iter()
            .map(|item| {
                item.into_node(
                    self.style.button_palette,
                    self.style.destructive_button_palette,
                    self.style.button_border_radius,
                    icons,
                )
            })
            .collect();
        if let Some(prefix) = self.prefix {
            children.insert(0, prefix);
        }
        if let Some(overflow) = self.overflow {
            children.push(LayoutContainer::spacer().into());
            let mut trigger = overflow.trigger();
            trigger.hovered = Some(trigger.action_key()) == self.hovered_action_key.as_ref();
            children.push(trigger.into_node(
                self.style.button_palette,
                self.style.destructive_button_palette,
                self.style.button_border_radius,
                icons,
            ));
        }
        LayoutContainer::row(children)
            .identity(IdentityConcern {
                key_strategy: KeyStrategy::AutoStableIdentity,
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::percent(1.0)),
                height: Some(Size::fixed(self.style.height)),
                gap: self.style.gap,
                padding: self.style.padding,
                border_width: self.style.border_width.max(0.0),
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.style.background)),
                border_color: self.style.border_color,
                border_radius: self.style.border_radius,
                ..VisualConcern::default()
            })
            .into()
    }
}

fn action_identity(action_key: SemanticKey) -> IdentityConcern {
    IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(action_key),
        key_segment: None,
        update_mode: UpdateMode::Dirty,
    }
}

#[cfg(test)]
mod tests {
    use super::{Toolbar, ToolbarItem, ToolbarOverflow, ToolbarStyle};
    use tela_contract::{
        BorderRadius, Color, IconName, IconOpticalMetrics, IconProvider, IconRequest,
        IconResolveError, IconVisual, SemanticKey, TextContent, TextStyleRef,
    };
    use tela_core::{IdentityAllocator, Primitive, UiTree};

    struct TestIcons;

    impl IconProvider for TestIcons {
        fn resolve(&self, request: IconRequest) -> Result<IconVisual, IconResolveError> {
            Ok(IconVisual::new(
                Primitive::text(TextContent {
                    text: request.key.as_str().to_owned(),
                    font: TextStyleRef::icon(),
                    font_size: request.size,
                    line_height: request.size,
                    color: Color::WHITE,
                })
                .into(),
                IconOpticalMetrics {
                    box_size: request.size,
                    ink_center_y: request.size * 0.5,
                },
            ))
        }
    }

    fn key_for_action(tree: &UiTree, action_key: &str) -> SemanticKey {
        tree.keys()
            .iter()
            .find(|key| key.0 == action_key)
            .cloned()
            .expect("Toolbar 项目应有稳定 semantic key")
    }

    #[test]
    fn enabled_item_exposes_a_clickable_semantic_action_key() {
        let node = Toolbar::new()
            .item(ToolbarItem::new("刷新", "command.refresh").width(64.0))
            .into_node(&TestIcons);
        let button = &node.children[0];
        assert!(
            button
                .interact
                .as_ref()
                .is_some_and(|interact| interact.clickable)
        );
        assert_eq!(
            button
                .identity
                .as_ref()
                .and_then(|identity| identity.semantic_key.as_ref())
                .map(|key| key.0.as_str()),
            Some("command.refresh")
        );
    }

    #[test]
    fn disabled_item_has_no_click_binding() {
        let node = Toolbar::new()
            .item(ToolbarItem::new("删除", "command.delete").disabled(true))
            .into_node(&TestIcons);
        assert!(node.children[0].interact.is_none());
    }

    #[test]
    fn hover_snapshot_changes_only_the_matching_item_visual_state() {
        let node = Toolbar::new()
            .item(ToolbarItem::new("刷新", "command.refresh"))
            .item(ToolbarItem::new("同步", "command.sync"))
            .hovered_action_key(Some(&SemanticKey("command.sync".to_owned())))
            .into_node(&TestIcons);
        let first = node.children[0]
            .visual
            .as_ref()
            .and_then(|v| v.fill.as_ref());
        let second = node.children[1]
            .visual
            .as_ref()
            .and_then(|v| v.fill.as_ref());
        assert_ne!(first, second);
    }

    #[test]
    fn overflow_keeps_its_items_and_exposes_a_distinct_trigger() {
        let overflow = ToolbarOverflow::new(
            "更多",
            "toolbar.more",
            vec![ToolbarItem::new("导出", "command.export")],
        );
        assert_eq!(overflow.items().len(), 1);
        assert_eq!(overflow.action_key().0, "toolbar.more");

        let node = Toolbar::new().overflow(overflow).into_node(&TestIcons);
        let trigger = node.children.last().expect("overflow trigger");
        assert_eq!(
            trigger
                .identity
                .as_ref()
                .and_then(|identity| identity.semantic_key.as_ref())
                .map(|key| key.0.as_str()),
            Some("toolbar.more")
        );
    }

    #[test]
    fn conditional_items_keep_core_identity_without_page_keys() {
        let mut allocator = IdentityAllocator::new();
        let first = UiTree::new_with_allocator(
            Toolbar::new()
                .item(ToolbarItem::new("新建", "command.new-folder"))
                .item(ToolbarItem::new("重命名", "command.rename"))
                .item(ToolbarItem::new("列表", "command.toggle-view"))
                .into_node(&TestIcons),
            &mut allocator,
        )
        .expect("Toolbar 应构成合法树");
        let rename_key = key_for_action(&first, "command.rename");
        let view_key = key_for_action(&first, "command.toggle-view");

        let second = UiTree::new_with_allocator(
            Toolbar::new()
                .item(ToolbarItem::new("新建", "command.new-folder"))
                .item(ToolbarItem::new("列表", "command.toggle-view"))
                .into_node(&TestIcons),
            &mut allocator,
        )
        .expect("条件收缩后的 Toolbar 应构成合法树");

        assert!(
            !second.keys().contains(&rename_key),
            "被卸载命令的 key 不能复用给后续项目"
        );
        assert_eq!(
            key_for_action(&second, "command.toggle-view"),
            view_key,
            "同一命令移动后仍由 core 保持其身份"
        );
    }

    #[test]
    fn style_applies_surface_and_command_corner_radii() {
        let surface_radius = BorderRadius::all(8.0);
        let node = Toolbar::new()
            .item(ToolbarItem::new("新建", "command.new").icon(IconName::Add))
            .style(ToolbarStyle {
                border_radius: surface_radius,
                border_color: Some(Color::BLUE),
                border_width: 1.0,
                button_border_radius: 6.0,
                ..ToolbarStyle::default()
            })
            .into_node(&TestIcons);

        assert_eq!(
            node.visual.as_ref().map(|visual| visual.border_radius),
            Some(surface_radius)
        );
        assert_eq!(
            node.layout.as_ref().map(|layout| layout.border_width),
            Some(1.0)
        );
        assert_eq!(
            node.children[0]
                .visual
                .as_ref()
                .map(|visual| visual.border_radius),
            Some(BorderRadius::all(6.0))
        );
    }
}
