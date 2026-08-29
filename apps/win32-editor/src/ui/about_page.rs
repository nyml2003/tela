//! 关于页与构建信息行组件（经静态路径桥查询，构造时缓存）。

use tela_contract::{Insets, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{Body, DslComponent, Signal, ViewBuild, ViewOutput, ViewResult, ui};

use super::theme::{CONTENT_INSET, SECONDARY, TEXT, TITLE_BAR_H};

/// 关于页：构建信息（经静态路径桥查询，构造时缓存为一次性节点——恒定数据
/// 以 set-once Signal 承载：有身份、永不脏）。
#[derive(DslComponent)]
pub struct AboutPage {
    #[watch]
    pub viewport: Signal<Viewport>,
    #[watch]
    pub rows: Signal<Vec<(String, String)>>,
}

impl AboutPage {
    /// 组件渲染（由 `DslComponent::render` 脚手架调用）。
    pub fn view<A>(
        &self,
        build: &mut ViewBuild<A>,
        _children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let viewport = self.viewport.get();
        let site = tela_ui_dsl::ViewSite::new(file!(), line!(), column!());
        let mut children = Vec::new();
        let rows = self.rows.get();
        for (label, value) in &rows {
            let row = ui!(build {
                <Row key={format!("editor.about.row.{label}")} gap={8.0}>
                    <Text value={format!("{label}:")} font_size={14.0} color={SECONDARY} />
                    <Text value={value.clone()} font_size={14.0} color={TEXT} />
                </Row>
            })?;
            children.push(tela_ui_dsl::into_view_child(row)?);
        }
        ui!(build {
            <Column
                key={"editor.about"}
                width={viewport.width}
                height={viewport.height - TITLE_BAR_H}
                padding={Insets { top: 24.0, right: CONTENT_INSET, bottom: 0.0, left: CONTENT_INSET }}
                gap={12.0}
            >
                <Text value={"关于"} font_size={20.0} color={TEXT} />
                <Text value={"Tela 文本编辑器 — Win32 静态 DSL 演示"} font_size={14.0} color={SECONDARY} />
                <Column key={"editor.about.rows"} gap={8.0}>
                    { build.fragment(Body::new(children, Vec::new()), site)? }
                </Column>
            </Column>
        })
    }
}
