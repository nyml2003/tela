//! 设置页布局容器组件与步进按钮（EditorAction 上下文组合）。

use tela_contract::{
    Color, Fill, FontDescriptor, Insets, PixelOffset, ShadowSpec, TextStyleRef, Viewport,
};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, DslComponent, Easing, Signal, TransitionSpec, ViewBuild, ViewOutput, ViewResult, ui,
};
use tela_ui_foundation::{Button, ButtonState};

use crate::application::EditorAction;

use super::theme::{BAR_BORDER, CONTENT_INSET, NAV_PALETTE, SECONDARY, TEXT, TITLE_BAR_H};

/// 设置页布局容器：固定标题/分隔线 + children 为调用点组合的步进行。
#[derive(DslComponent)]
pub struct SettingsPage {
    #[watch]
    pub viewport: Signal<Viewport>,
}

impl SettingsPage {
    /// 组件渲染（由 `DslComponent::render` 脚手架调用）。
    pub fn view<A>(
        &self,
        build: &mut ViewBuild<A>,
        children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let viewport = self.viewport.get();
        ui!(build {
            <Column
                key={"editor.settings"}
                width={viewport.width}
                height={viewport.height - TITLE_BAR_H}
                padding={Insets { top: 24.0, right: CONTENT_INSET, bottom: 0.0, left: CONTENT_INSET }}
                gap={16.0}
            >
                <Text value={"设置"} font={TextStyleRef::body_medium()} font_size={20.0} color={TEXT} />
                <Text value={"编辑区字体"} font={TextStyleRef::body_medium()} font_size={14.0} color={SECONDARY} />
                <View key={"editor.divider.font"} width={viewport.width - CONTENT_INSET * 2.0}
                      height={1.0} fill={Fill::Solid(BAR_BORDER)} />
                { build.fragment(children, tela_ui_dsl::ViewSite::new(file!(), line!(), column!()))? }
            </Column>
        })
    }
}

/// 字体目录项：选择动作由调用点组合，字体预览自身使用该项 token。
pub fn font_item(
    build: &mut ViewBuild<EditorAction>,
    font: &FontDescriptor,
    selected: &TextStyleRef,
    hover_key: &Option<String>,
) -> ViewResult<ViewOutput<EditorAction>> {
    let hovered = hover_key.as_deref().is_some_and(|key| {
        key.contains("/@for-") && key.rsplit('/').next() == Some(font.text_style)
    });
    let choice = font_choice_node(
        build,
        font.display_name,
        &TextStyleRef::new(font.text_style),
        font.weight,
        selected.as_str() == font.text_style,
        hovered,
    )?;
    ui!(build {
        <ActionTarget action={EditorAction::SetFont(TextStyleRef::new(font.text_style))}>
            { choice }
        </ActionTarget>
    })
}

/// 字体选择按钮节点（hover 为宿主帧级值；transition 由 Frame 自身推进）。
fn font_choice_node<A>(
    build: &mut ViewBuild<A>,
    label: &str,
    font: &TextStyleRef,
    weight: u16,
    selected: bool,
    hovered: bool,
) -> ViewResult<ViewOutput<A>> {

    let fill = if selected {
            Color::rgba(0.82, 0.91, 1.0, 1.0)
        } else if hovered {
            Color::rgba(0.94, 0.97, 1.0, 1.0)
        } else {
            Color::rgba(1.0, 1.0, 1.0, 0.92)
        };
        let border = if selected {
            Color::rgba(0.16, 0.48, 0.82, 1.0)
        } else {
            Color::rgba(0.78, 0.82, 0.88, 1.0)
        };
        ui!(build {
            <Frame
                width={202.0}
                height={42.0}
                padding={Insets::all(10.0)}
                border_width={1.0}
                border_radius={6.0}
                fill={Fill::Solid(fill)}
                border_color={border}
                shadow={ShadowSpec {
                    offset: PixelOffset { x: 0.0, y: 2.0 },
                    blur_radius: if selected { 7.0 } else { 4.0 },
                    color: Color::rgba(0.05, 0.10, 0.18, if selected { 0.18 } else { 0.08 }),
                    inset: false,
                }}
                transition={TransitionSpec::new(150, Easing::STANDARD)}
                clickable={true}
                hoverable={true}
            >
                <Row gap={8.0} cross_align={tela_contract::CrossAlign::Center}>
                    <Text value={label} font={font.clone()} font_size={13.0} color={TEXT} />
                    <Text value={format!("{}", weight)} font={TextStyleRef::body_medium()} font_size={11.0} color={SECONDARY} opacity={0.82} />
                </Row>
            </Frame>
        })
}

/// 设置页步进项：ActionTarget（宏内建）+ 步进按钮节点（EditorAction 上下文）。
pub fn step_item(
    build: &mut ViewBuild<EditorAction>,
    key_suffix: &str,
    label: &str,
    action: EditorAction,
    hover_key: &Option<String>,
) -> ViewResult<ViewOutput<EditorAction>> {
    let key = format!("editor.step.{key_suffix}");
    let hovered = hover_key.as_deref() == Some(key.as_str());
    let button = step_button_node(key, label, hovered);
    ui!(build {
        <ActionTarget action={action}>
            { button }
        </ActionTarget>
    })
}

/// 步进按钮节点（foundation Button 包装，无边框）。
fn step_button_node<A>(key: String, label: &str, hovered: bool) -> ViewOutput<A> {
    let mut node = Button::new(label)
        .width(64.0)
        .height(28.0)
        .border_radius(4.0)
        .text_metrics(13.0, 18.0)
        .state(ButtonState {
            hovered,
            selected: false,
            disabled: false,
        })
        .palette(NAV_PALETTE)
        .into_node();
    node.identity = Some(tela_contract::IdentityConcern {
        key_strategy: tela_contract::KeyStrategy::SemanticId,
        semantic_key: Some(tela_contract::SemanticKey(key)),
        key_segment: None,
        update_mode: tela_contract::UpdateMode::Dirty,
    });
    ViewOutput::opaque(node)
}

