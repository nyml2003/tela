//! 主题无关的命令工具栏。

use tela_contract::{
    Color, Fill, IdentityConcern, Insets, KeyStrategy, LayoutConcern, Size, UiNode, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};
use tela_widgets::{ButtonPalette, IconButton, IconButtonPalette, IconButtonVariant, IconName};

use crate::intent::IntentTarget;

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
            button_palette: None,
            destructive_button_palette: None,
        }
    }
}

/// 工具栏中的一个可执行项。
#[derive(Clone, Debug, PartialEq)]
pub struct ToolbarItem {
    label: String,
    target: IntentTarget,
    icon: Option<IconName>,
    show_label: bool,
    disabled: bool,
    destructive: bool,
    hovered: bool,
    width: f32,
}

impl ToolbarItem {
    /// 创建一个普通命令项。
    pub fn new(label: impl Into<String>, target: impl Into<IntentTarget>) -> Self {
        Self {
            label: label.into(),
            target: target.into(),
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

    /// 设置禁用态。禁用项不会产生 `UiIntent`。
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

    /// 返回此项的业务意图目标。
    pub fn target(&self) -> &IntentTarget {
        &self.target
    }

    fn into_node(
        self,
        palette: Option<ButtonPalette>,
        destructive_palette: Option<ButtonPalette>,
    ) -> UiNode {
        let mut button = if let Some(icon) = self.icon {
            let mut value = IconButton::new(icon)
                .size(self.width, 30.0)
                .variant(if self.destructive {
                    IconButtonVariant::Danger
                } else {
                    IconButtonVariant::Primary
                })
                .state(tela_widgets::IconButtonState {
                    hovered: self.hovered,
                    selected: false,
                    disabled: self.disabled,
                });
            if self.show_label {
                value = value.label(self.label);
            }
            if let Some(palette) = if self.destructive {
                destructive_palette.map(icon_palette)
            } else {
                palette.map(icon_palette)
            } {
                value = value.palette(palette);
            }
            let mut node = value.into_node();
            if let Some(interact) = &mut node.interact {
                interact.bind_id = Some(self.target.bind_id());
            }
            return node;
        } else {
            // 兼容无图标的调用方，继续使用已有原子按钮。
            tela_widgets::Button::new(self.label)
                .width(self.width)
                .height(26.0)
                .variant(if self.destructive {
                    tela_widgets::ButtonVariant::Danger
                } else {
                    tela_widgets::ButtonVariant::Primary
                })
                .disabled(self.disabled)
                .hovered(self.hovered)
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
        if let Some(interact) = &mut node.interact {
            interact.bind_id = Some(self.target.bind_id());
        }
        node
    }
}

fn icon_palette(palette: ButtonPalette) -> IconButtonPalette {
    IconButtonPalette {
        normal: palette.normal,
        hovered: palette.hovered,
        selected: palette.selected,
        disabled: palette.disabled,
        text: palette.text,
        disabled_text: palette.disabled_text,
    }
}

/// 收纳在“更多”入口后的项目集合。
///
/// 本期不实现 Menu/Popover 的局部展开状态；宿主收到 [`ToolbarOverflow::target`] 的 `Invoke`
/// 后，可按 `items` 构建自己的菜单或在下一期接入 `tela-ui::Menu`。
#[derive(Clone, Debug, PartialEq)]
pub struct ToolbarOverflow {
    label: String,
    target: IntentTarget,
    items: Vec<ToolbarItem>,
    width: f32,
}

impl ToolbarOverflow {
    /// 创建溢出入口及其逻辑项目。
    pub fn new(
        label: impl Into<String>,
        target: impl Into<IntentTarget>,
        items: Vec<ToolbarItem>,
    ) -> Self {
        Self {
            label: label.into(),
            target: target.into(),
            items,
            width: 52.0,
        }
    }

    /// 设置入口宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// 返回溢出入口目标。
    pub fn target(&self) -> &IntentTarget {
        &self.target
    }

    /// 返回由宿主在后续菜单中消费的项目。
    pub fn items(&self) -> &[ToolbarItem] {
        &self.items
    }

    fn trigger(&self) -> ToolbarItem {
        ToolbarItem::new(self.label.clone(), self.target.clone()).width(self.width)
    }
}

/// 一行命令项与可选溢出入口。
pub struct Toolbar {
    items: Vec<ToolbarItem>,
    overflow: Option<ToolbarOverflow>,
    prefix: Option<UiNode>,
    style: ToolbarStyle,
    hovered_target: Option<String>,
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
            hovered_target: None,
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

    /// 以目标路由名向指定项目投影 core 的当前悬停状态。
    ///
    /// 业务页面不管理 tela key；该路由值仅用于本次受控 view 投影。
    pub fn hovered_target(mut self, target: Option<&str>) -> Self {
        for item in &mut self.items {
            item.hovered = Some(item.target.as_str()) == target;
        }
        self.hovered_target = target.map(str::to_owned);
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
    /// `AutoStableIdentity` 策略。调用方不提供、组件也不保存 tela key；实际身份仍由 core
    /// 根据稳定命令绑定分配。
    pub fn into_node(self) -> UiNode {
        let mut children: Vec<UiNode> = self
            .items
            .into_iter()
            .map(|item| {
                item.into_node(
                    self.style.button_palette,
                    self.style.destructive_button_palette,
                )
            })
            .collect();
        if let Some(prefix) = self.prefix {
            children.insert(0, prefix);
        }
        if let Some(overflow) = self.overflow {
            children.push(
                Primitive::rect()
                    .layout(LayoutConcern {
                        width: Some(Size::fill()),
                        height: Some(Size::fixed(1.0)),
                        ..LayoutConcern::default()
                    })
                    .into(),
            );
            let mut trigger = overflow.trigger();
            trigger.hovered = Some(trigger.target.as_str()) == self.hovered_target.as_deref();
            children.push(trigger.into_node(
                self.style.button_palette,
                self.style.destructive_button_palette,
            ));
        }
        LayoutContainer::flex(children)
            .identity(IdentityConcern {
                key_strategy: KeyStrategy::AutoStableIdentity,
                ..IdentityConcern::default()
            })
            .layout(LayoutConcern {
                width: Some(Size::fill()),
                height: Some(Size::fixed(self.style.height)),
                gap: self.style.gap,
                padding: self.style.padding,
                cross_align: tela_contract::CrossAlign::Center,
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.style.background)),
                ..VisualConcern::default()
            })
            .into()
    }
}

impl From<Toolbar> for UiNode {
    fn from(toolbar: Toolbar) -> Self {
        toolbar.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{Toolbar, ToolbarItem, ToolbarOverflow};
    use crate::{UiIntent, intent_from_action};
    use tela_contract::{NodeId, SemanticKey, UiAction};
    use tela_core::{IdentityAllocator, UiTree};

    fn key_for_target(tree: &UiTree, target: &str) -> SemanticKey {
        let bind_id = format!("ui.invoke:{target}");
        tree.keys()
            .iter()
            .find(|key| {
                tree.interact_for_key(key)
                    .and_then(|interact| interact.bind_id.as_ref())
                    .is_some_and(|candidate| candidate.0 == bind_id)
            })
            .cloned()
            .expect("Toolbar 项目应有稳定 core key")
    }

    #[test]
    fn enabled_item_maps_click_to_invoke() {
        let node = Toolbar::new()
            .item(ToolbarItem::new("刷新", "command.refresh").width(64.0))
            .into_node();
        let button = &node.children[0];
        let bind_id = button
            .interact
            .as_ref()
            .and_then(|interact| interact.bind_id.as_ref());
        assert_eq!(
            intent_from_action(&UiAction::Click { node_id: NodeId(1) }, bind_id),
            Some(UiIntent::Invoke {
                target: "command.refresh".into(),
            })
        );
    }

    #[test]
    fn disabled_item_has_no_click_binding() {
        let node = Toolbar::new()
            .item(ToolbarItem::new("删除", "command.delete").disabled(true))
            .into_node();
        assert!(node.children[0].interact.is_none());
    }

    #[test]
    fn hover_snapshot_changes_only_the_matching_item_visual_state() {
        let node = Toolbar::new()
            .item(ToolbarItem::new("刷新", "command.refresh"))
            .item(ToolbarItem::new("同步", "command.sync"))
            .hovered_target(Some("command.sync"))
            .into_node();
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
        assert_eq!(overflow.target().as_str(), "toolbar.more");

        let node = Toolbar::new().overflow(overflow).into_node();
        let trigger = node.children.last().expect("overflow trigger");
        let bind_id = trigger
            .interact
            .as_ref()
            .and_then(|interact| interact.bind_id.as_ref());
        assert_eq!(
            intent_from_action(&UiAction::Click { node_id: NodeId(2) }, bind_id),
            Some(UiIntent::Invoke {
                target: "toolbar.more".into(),
            })
        );
    }

    #[test]
    fn conditional_items_keep_core_identity_without_page_keys() {
        let mut allocator = IdentityAllocator::new();
        let first = UiTree::new_with_allocator(
            Toolbar::new()
                .item(ToolbarItem::new("新建", "command.new-folder"))
                .item(ToolbarItem::new("重命名", "command.rename"))
                .item(ToolbarItem::new("列表", "command.toggle-view")),
            &mut allocator,
        )
        .expect("Toolbar 应构成合法树");
        let rename_key = key_for_target(&first, "command.rename");
        let view_key = key_for_target(&first, "command.toggle-view");

        let second = UiTree::new_with_allocator(
            Toolbar::new()
                .item(ToolbarItem::new("新建", "command.new-folder"))
                .item(ToolbarItem::new("列表", "command.toggle-view")),
            &mut allocator,
        )
        .expect("条件收缩后的 Toolbar 应构成合法树");

        assert!(
            !second.keys().contains(&rename_key),
            "被卸载命令的 key 不能复用给后续项目"
        );
        assert_eq!(
            key_for_target(&second, "command.toggle-view"),
            view_key,
            "同一命令移动后仍由 core 保持其身份"
        );
    }
}
