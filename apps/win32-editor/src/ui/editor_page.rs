//! 编辑器页组件：多行文本输入区（字号/行距随设置）。

use tela_contract::{Fill, Insets, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{Body, DslComponent, ViewBuild, ViewOutput, ViewResult, ui};

use crate::application::EditorSettings;

use super::theme::{CONTENT_BACKGROUND, CONTENT_INSET, TEXT, TITLE_BAR_H};

/// 编辑器页：多行文本输入区（字号/行距随设置）。
#[derive(DslComponent)]
#[allow(missing_docs)]
pub struct EditorPage {
    pub viewport: Viewport,
    pub settings: EditorSettings,
    pub document: String,
}

impl EditorPage {
    /// 组件渲染（由 `DslComponent::render` 脚手架调用）。
    pub fn view<A>(
        &self,
        build: &mut ViewBuild<A>,
        _children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let font_size = self.settings.font_size as f32;
        let line_height = font_size * (self.settings.line_height as f32 / 100.0);
        let content_width = self.viewport.width - CONTENT_INSET * 2.0;
        let content_height = self.viewport.height - TITLE_BAR_H - CONTENT_INSET * 2.0;
        ui!(build {
            <ScrollView
                key={"editor.page.scroll"}
                width={self.viewport.width}
                height={self.viewport.height - TITLE_BAR_H}
                padding={Insets { top: CONTENT_INSET, right: CONTENT_INSET, bottom: CONTENT_INSET, left: CONTENT_INSET }}
                overflow={tela_contract::Overflow::Scroll}
                clip={true}
            >
                <Frame
                    key={"editor.page.field"}
                    width={content_width}
                    height={content_height}
                    input={tela_contract::TextInputSpec::new(tela_contract::TextInputKind::Multiline).value(self.document.clone())}
                    fill={Fill::Solid(CONTENT_BACKGROUND)}
                    clickable={true}
                    focusable={true}
                >
                    <Text
                        value={self.document.clone()}
                        font_size={font_size}
                        line_height={line_height}
                        color={TEXT}
                    />
                </Frame>
            </ScrollView>
        })
    }
}
