//! 自绘标题栏组件（导航 + 窗口控制一行式）。

use tela_contract::{
    CrossAlign, Fill, IconName, IconProvider, IconRequest, IdentityConcern, Insets, KeyStrategy,
    SemanticKey, UpdateMode, WindowCommand,
};
use tela_icon_resources::MaterialIconFontProvider;
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{Body, DslComponent, Signal, ViewBuild, ViewOutput, ViewResult, ui};
use tela_ui_foundation::{Button, ButtonPalette, ButtonState};

use crate::application::EditorAction;

use super::theme::{
    BAR_BACKGROUND, BAR_BORDER, CLOSE_HOVER, NAV_PALETTE, SECONDARY, TEXT, TITLE_BAR_H,
};

/// 关闭按钮调色板（hover 红色，Win11 惯例）。
const CLOSE_PALETTE: ButtonPalette = ButtonPalette {
    normal: BAR_BACKGROUND,
    hovered: CLOSE_HOVER,
    selected: CLOSE_HOVER,
    disabled: BAR_BACKGROUND,
    text: TEXT,
    disabled_text: SECONDARY,
};

/// 自绘标题栏：导航按钮 + 窗口控制按钮一行式。
///
/// children 为调用点组合的导航项 + 窗口按钮（EditorAction 上下文）。
/// 携带 children → 自动退出 retained（001 §2）。
#[derive(DslComponent)]
pub struct TitleBar {
    #[watch]
    pub viewport: Signal<tela_contract::Viewport>,
}

impl TitleBar {
    /// 组件渲染（由 `DslComponent::render` 脚手架调用）。
    pub fn view<A>(
        &self,
        build: &mut ViewBuild<A>,
        children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        ui!(build {
            <Row
                key={"editor.titlebar"}
                width={self.viewport.get().width}
                height={TITLE_BAR_H}
                padding={Insets { top: 0.0, right: 0.0, bottom: 0.0, left: 8.0 }}
                gap={4.0}
                cross_align={CrossAlign::Center}
                fill={Fill::Solid(BAR_BACKGROUND)}
                border_width={1.0}
                border_color={BAR_BORDER}
                window_drag_region={true}
            >
                { build.fragment(children, tela_ui_dsl::ViewSite::new(file!(), line!(), column!()))? }
            </Row>
        })
    }
}

/// 窗口控制项：ActionTarget + WindowButton 组件（EditorAction 上下文）。
pub fn window_item(
    build: &mut ViewBuild<EditorAction>,
    command: WindowCommand,
    key_suffix: &str,
    window_maximized: bool,
    hover_key: &Option<String>,
) -> ViewResult<ViewOutput<EditorAction>> {
    let key = format!("editor.window.{key_suffix}");
    let hovered = hover_key.as_deref() == Some(key.as_str());
    let icon = window_icon(command, window_maximized);
    let button = window_button_node(key, icon, command == WindowCommand::Close, hovered);
    ui!(build {
        <ActionTarget action={EditorAction::Window(command)}>
            { button }
        </ActionTarget>
    })
}

fn window_icon(command: WindowCommand, window_maximized: bool) -> IconName {
    match command {
        WindowCommand::Minimize => IconName::Minimize,
        WindowCommand::Maximize => {
            if window_maximized {
                IconName::WindowRestore
            } else {
                IconName::Maximize
            }
        }
        WindowCommand::Close => IconName::Close,
    }
}

/// 窗口控制按钮节点（foundation Button 包装 Material 图标；hover 为宿主帧级值）。
fn window_button_node<A>(key: String, icon: IconName, close: bool, hovered: bool) -> ViewOutput<A> {
    let glyph = MaterialIconFontProvider
        .resolve(IconRequest {
            key: icon.into(),
            size: 18.0,
            color: TEXT,
        })
        .expect("标准窗口控制图标必须可解析")
        .into_node();
    let mut node = Button::new("")
        .content(glyph)
        .width(40.0)
        .height(TITLE_BAR_H - 8.0)
        .border_radius(0.0)
        .state(ButtonState {
            hovered,
            selected: false,
            disabled: false,
        })
        .palette(if close { CLOSE_PALETTE } else { NAV_PALETTE })
        .into_node();
    node.identity = Some(IdentityConcern {
        key_strategy: KeyStrategy::SemanticId,
        semantic_key: Some(SemanticKey(key)),
        key_segment: None,
        update_mode: UpdateMode::Dirty,
    });
    ViewOutput::opaque(node)
}

#[cfg(test)]
mod tests {
    use super::window_icon;
    use tela_contract::{IconName, WindowCommand};

    #[test]
    fn maximize_control_switches_between_maximize_and_restore_icons() {
        assert_eq!(
            window_icon(WindowCommand::Maximize, false),
            IconName::Maximize
        );
        assert_eq!(
            window_icon(WindowCommand::Maximize, true),
            IconName::WindowRestore
        );
    }
}
