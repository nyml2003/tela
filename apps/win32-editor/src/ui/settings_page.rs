//! 设置页布局容器组件与步进按钮（EditorAction 上下文组合）。

use tela_contract::{Fill, Insets, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{Body, DslComponent, ViewBuild, ViewOutput, ViewResult, ui};
use tela_ui_foundation::{Button, ButtonState};

use crate::application::EditorAction;

use super::theme::{BAR_BORDER, CONTENT_INSET, NAV_PALETTE, SECONDARY, TEXT, TITLE_BAR_H};

/// 设置页布局容器：固定标题/分隔线 + children 为调用点组合的步进行。
#[derive(DslComponent)]
#[allow(missing_docs)]
pub struct SettingsPage {
    pub viewport: Viewport,
}

impl SettingsPage {
    /// 组件渲染（由 `DslComponent::render` 脚手架调用）。
    pub fn view<A>(
        &self,
        build: &mut ViewBuild<A>,
        children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        ui!(build {
            <Column
                key={"editor.settings"}
                width={self.viewport.width}
                height={self.viewport.height - TITLE_BAR_H}
                padding={Insets { top: 24.0, right: CONTENT_INSET, bottom: 0.0, left: CONTENT_INSET }}
                gap={16.0}
            >
                <Text value={"设置"} font_size={20.0} color={TEXT} />
                <Text value={"字体大小"} font_size={14.0} color={SECONDARY} />
                <View key={"editor.divider.font"} width={self.viewport.width - CONTENT_INSET * 2.0}
                      height={1.0} fill={Fill::Solid(BAR_BORDER)} />
                { build.fragment(children, tela_ui_dsl::ViewSite::new(file!(), line!(), column!()))? }
            </Column>
        })
    }
}

/// 设置页步进项：ActionTarget（宏内建）+ StepButton 组件（EditorAction 上下文）。
pub fn step_item(
    build: &mut ViewBuild<EditorAction>,
    key_suffix: &str,
    label: &str,
    action: EditorAction,
    hover_key: &Option<String>,
) -> ViewResult<ViewOutput<EditorAction>> {
    let key = format!("editor.step.{key_suffix}");
    let hovered = hover_key.as_deref() == Some(key.as_str());
    ui!(build {
        <ActionTarget action={action}>
            <StepButton key={key} label={label} hovered={hovered} />
        </ActionTarget>
    })
}

/// 设置页步进按钮组件（foundation Button 包装，无边框）。
#[derive(DslComponent)]
#[allow(missing_docs)]
pub struct StepButton {
    pub key: Option<String>,
    pub label: String,
    pub hovered: bool,
}

impl StepButton {
    /// 组件渲染（由 `DslComponent::render` 脚手架调用）。
    pub fn view<A>(
        &self,
        _build: &mut ViewBuild<A>,
        _children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let mut node = Button::new(&self.label)
            .width(64.0)
            .height(28.0)
            .border_radius(4.0)
            .text_metrics(13.0, 18.0)
            .state(ButtonState {
                hovered: self.hovered,
                selected: false,
                disabled: false,
            })
            .palette(NAV_PALETTE)
            .into_node();
        if let Some(key) = &self.key {
            node.identity = Some(tela_contract::IdentityConcern {
                key_strategy: tela_contract::KeyStrategy::SemanticId,
                semantic_key: Some(tela_contract::SemanticKey(key.clone())),
                key_segment: None,
                update_mode: tela_contract::UpdateMode::Dirty,
            });
        }
        Ok(ViewOutput::opaque(node))
    }
}
