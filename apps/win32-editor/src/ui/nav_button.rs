//! 导航按钮组件（foundation Button 包装）与导航项组合（EditorAction 上下文）。

use tela_contract::{IdentityConcern, KeyStrategy, SemanticKey, UpdateMode};
use tela_ui_dsl::{Body, DslComponent, ViewBuild, ViewOutput, ViewResult, ui};
use tela_ui_foundation::{Button, ButtonState};

use crate::application::{EditorAction, Route};

use super::theme::NAV_PALETTE;

/// 导航项：ActionTarget（宏内建）+ NavButton 组件（EditorAction 上下文）。
pub fn nav_item(
    build: &mut ViewBuild<EditorAction>,
    target: Route,
    label: &str,
    current: Route,
    hover_key: &Option<String>,
) -> ViewResult<ViewOutput<EditorAction>> {
    let key = format!("win32.nav.{}", route_name(target));
    let hovered = hover_key.as_deref() == Some(key.as_str());
    let selected = target == current;
    ui!(build {
        <ActionTarget action={EditorAction::Navigate(target)}>
            <NavButton
                key={key}
                label={label}
                selected={selected}
                hovered={hovered}
            />
        </ActionTarget>
    })
}

/// 导航按钮组件：foundation Button 包装，文本居中由 Button 内部
/// `Row + [Spacer, content, Spacer]` + `cross_align: Center` 布局保证。
#[derive(DslComponent)]
#[allow(missing_docs)]
pub struct NavButton {
    pub key: Option<String>,
    pub label: String,
    #[prop(default = 72.0)]
    pub width: f32,
    pub selected: bool,
    pub hovered: bool,
}

impl NavButton {
    /// 组件渲染（由 `DslComponent::render` 脚手架调用）。
    pub fn view<A>(
        &self,
        _build: &mut ViewBuild<A>,
        _children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let mut node = Button::new(&self.label)
            .width(self.width)
            .height(30.0)
            .border_radius(4.0)
            .text_metrics(13.0, 18.0)
            .state(ButtonState {
                hovered: self.hovered,
                selected: self.selected,
                disabled: false,
            })
            .palette(NAV_PALETTE)
            .into_node();
        // 语义键使 hover_key（内核 view_state）能稳定匹配到本按钮。
        if let Some(key) = &self.key {
            node.identity = Some(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(SemanticKey(key.clone())),
                key_segment: None,
                update_mode: UpdateMode::Dirty,
            });
        }
        Ok(ViewOutput::opaque(node))
    }
}

fn route_name(route: Route) -> &'static str {
    match route {
        Route::Editor => "editor",
        Route::Settings => "settings",
        Route::About => "about",
    }
}
