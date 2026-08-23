//! 关于页与构建信息行组件（经静态路径桥查询，构造时缓存）。

use tela_contract::{Insets, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{Body, DslComponent, ViewBuild, ViewOutput, ViewResult, ui};

use super::theme::{CONTENT_INSET, SECONDARY, TEXT, TITLE_BAR_H};

/// 关于页：构建信息（经静态路径桥查询，构造时缓存）。
#[derive(DslComponent)]
#[allow(missing_docs)]
pub struct AboutPage {
    pub viewport: Viewport,
    pub rows: Vec<(String, String)>,
}

impl AboutPage {
    /// 组件渲染（由 `DslComponent::render` 脚手架调用）。
    pub fn view<A>(
        &self,
        build: &mut ViewBuild<A>,
        _children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        ui!(build {
            <Column
                key={"editor.about"}
                width={self.viewport.width}
                height={self.viewport.height - TITLE_BAR_H}
                padding={Insets { top: 24.0, right: CONTENT_INSET, bottom: 0.0, left: CONTENT_INSET }}
                gap={12.0}
            >
                <Text value={"关于"} font_size={20.0} color={TEXT} />
                <Text value={"Tela 文本编辑器 — Win32 静态 DSL 演示"} font_size={14.0} color={SECONDARY} />
                <AboutRows rows={self.rows.clone()} />
            </Column>
        })
    }
}

/// 关于页构建信息行（标签 + 值）。
#[derive(DslComponent)]
#[allow(missing_docs)]
pub struct AboutRows {
    pub rows: Vec<(String, String)>,
}

impl AboutRows {
    /// 组件渲染（由 `DslComponent::render` 脚手架调用）。
    pub fn view<A>(
        &self,
        build: &mut ViewBuild<A>,
        _children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = tela_ui_dsl::ViewSite::new(file!(), line!(), column!());
        let mut children = Vec::new();
        for (label, value) in &self.rows {
            let row = ui!(build {
                <Row key={format!("editor.about.row.{label}")} gap={8.0}>
                    <Text value={format!("{label}:")} font_size={14.0} color={SECONDARY} />
                    <Text value={value.clone()} font_size={14.0} color={TEXT} />
                </Row>
            })?;
            children.push(tela_ui_dsl::into_view_child(row)?);
        }
        ui!(build {
            <Column key={"editor.about.rows"} gap={8.0}>
                { build.fragment(Body::new(children, Vec::new()), site)? }
            </Column>
        })
    }
}
