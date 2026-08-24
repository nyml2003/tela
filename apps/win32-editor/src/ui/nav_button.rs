//! 导航按钮组件（foundation Button 包装）与导航项组合（EditorAction 上下文）。

use tela_contract::{Color, Fill, Insets, PixelOffset, TextStyleRef};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, DslComponent, Easing, TransitionSpec, ViewBuild, ViewOutput, ViewResult, ui,
};

use crate::application::{EditorAction, Route};

use super::theme::NAV_PALETTE;

/// 导航项：ActionTarget（宏内建）+ NavButton 组件（EditorAction 上下文）。
pub fn nav_item(
    build: &mut ViewBuild<EditorAction>,
    target: Route,
    label: &str,
    current: Route,
    hover_key: &Option<String>,
    pressed_key: &Option<String>,
) -> ViewResult<ViewOutput<EditorAction>> {
    let key = format!("editor.nav.{}", route_name(target));
    let hovered = hover_key.as_deref() == Some(key.as_str());
    let selected = target == current;
    let pressed = pressed_key.as_deref() == Some(key.as_str());
    ui!(build {
        <ActionTarget action={EditorAction::Navigate(target)}>
            <NavButton
                key={key}
                label={label}
                selected={selected}
                hovered={hovered}
                pressed={pressed}
            />
        </ActionTarget>
    })
}

/// 导航按钮组件：使用 `ui!` 原语声明，视觉变化由组件 owner 内的隐式 transition 推进。
#[derive(DslComponent)]
#[allow(missing_docs)]
pub struct NavButton {
    pub key: Option<String>,
    pub label: String,
    #[prop(default = 72.0)]
    pub width: f32,
    pub selected: bool,
    pub hovered: bool,
    pub pressed: bool,
}

impl NavButton {
    /// 组件渲染（由 `DslComponent::render` 脚手架调用）。
    pub fn view<A>(
        &self,
        build: &mut ViewBuild<A>,
        _children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let fill = if self.pressed {
            Color::rgba(0.72, 0.84, 0.94, 1.0)
        } else if self.selected {
            NAV_PALETTE.selected
        } else if self.hovered {
            NAV_PALETTE.hovered
        } else {
            NAV_PALETTE.normal
        };
        ui!(build {
            <Frame
                key={self.key.clone().unwrap_or_else(|| "editor.nav.item".to_owned())}
                width={self.width}
                height={30.0}
                padding={Insets { top: 6.0, right: 8.0, bottom: 6.0, left: 8.0 }}
                border_radius={5.0}
                fill={Fill::Solid(fill)}
                visual_offset={if self.pressed { PixelOffset { x: 0.0, y: 1.0 } } else { PixelOffset::default() }}
                clickable={true}
                hoverable={true}
                transition={TransitionSpec::new(140, Easing::STANDARD)}
            >
                <Row cross_align={tela_contract::CrossAlign::Center}>
                    { tela_ui_dsl::into_view_child::<A, tela_contract::UiNode>(tela_core::LayoutContainer::spacer().into())? }
                    <Text value={self.label.clone()} font={TextStyleRef::body_medium()} font_size={13.0} line_height={18.0} color={NAV_PALETTE.text} />
                    { tela_ui_dsl::into_view_child::<A, tela_contract::UiNode>(tela_core::LayoutContainer::spacer().into())? }
                </Row>
            </Frame>
        })
    }
}

fn route_name(route: Route) -> &'static str {
    match route {
        Route::Editor => "editor",
        Route::Icons => "icons",
        Route::Settings => "settings",
        Route::About => "about",
    }
}
