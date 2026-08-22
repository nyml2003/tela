//! Material Symbols Rounded iconfont provider。

use tela_contract::{
    Color, ContentConcern, IconName, IconOpticalMetrics, IconProvider, IconRequest,
    IconResolveError, IconVisual, NodeKind, TextContent, TextStyleRef, UiNode,
};
use tela_text_resources::glyph_ink_metrics;

/// 内建 Material Symbols Rounded provider。
///
/// 这是 Presentation 的资源实现。产品根显式将它与文本度量器组合后注入 Application；
/// UI kit 不应直接构造它。
#[derive(Clone, Copy, Debug, Default)]
pub struct MaterialIconFontProvider;

impl IconProvider for MaterialIconFontProvider {
    fn resolve(&self, request: IconRequest) -> Result<IconVisual, IconResolveError> {
        let Some(name) = IconName::from_key(request.key.as_str()) else {
            return Err(IconResolveError { key: request.key });
        };
        Ok(resolve_material_icon(name, request.size, request.color))
    }
}

fn resolve_material_icon(name: IconName, size: f32, color: Color) -> IconVisual {
    let size = size.max(1.0);
    let content = TextContent {
        text: material_codepoint(name).to_string(),
        font: TextStyleRef::icon(),
        font_size: size,
        line_height: size,
        color,
    };
    let ink_center_y = glyph_ink_metrics(&content)
        .map(|metrics| metrics.center_y())
        .unwrap_or(size * 0.5);
    IconVisual::new(
        UiNode::new(NodeKind::Text).with_content(ContentConcern::Text(content)),
        IconOpticalMetrics {
            box_size: size,
            ink_center_y,
        },
    )
}

/// Material Symbols Rounded 的资源私有码位表。
///
/// 新图标应先被确认为通用 UI 语义或业务私有 [`IconKey`]，再在本实现中补充资源映射；
/// 不要把此表反向暴露给 Application。
fn material_codepoint(name: IconName) -> char {
    match name {
        IconName::Add => '\u{e145}',
        IconName::Delete => '\u{e92e}',
        IconName::Edit => '\u{f097}',
        IconName::Copy => '\u{e14d}',
        IconName::Move => '\u{e9a1}',
        IconName::Restore => '\u{e938}',
        IconName::Favorite => '\u{f09a}',
        IconName::Tag => '\u{e893}',
        IconName::Undo => '\u{e166}',
        IconName::Search => '\u{ef7a}',
        IconName::Folder => '\u{e2c7}',
        IconName::FolderOpen => '\u{e2c8}',
        IconName::Document => '\u{e873}',
        IconName::Image => '\u{e3f4}',
        IconName::Archive => '\u{eb2c}',
        IconName::AllFiles => '\u{e9b2}',
        IconName::Trash => '\u{e92e}',
        IconName::List => '\u{e8ef}',
        IconName::Grid => '\u{e9b0}',
        IconName::Sort => '\u{e164}',
        IconName::Filter => '\u{e152}',
        IconName::ChevronRight => '\u{e5cc}',
        IconName::ArrowBack => '\u{e5c4}',
        IconName::Menu => '\u{e5d2}',
        IconName::More => '\u{e5d3}',
        IconName::Close => '\u{e5cd}',
        IconName::Minimize => '\u{e931}',
        IconName::Maximize => '\u{e92c}',
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{
        Color, ContentConcern, IconKey, IconName, IconProvider, IconRequest, NodeKind, TextStyleRef,
    };

    use super::MaterialIconFontProvider;

    #[test]
    fn provider_maps_every_standard_semantic_icon_to_the_controlled_icon_font() {
        let provider = MaterialIconFontProvider;
        for name in IconName::ALL {
            let node = provider
                .resolve(IconRequest {
                    key: (*name).into(),
                    size: 20.0,
                    color: Color::WHITE,
                })
                .expect("standard icon must resolve")
                .into_node();
            assert_eq!(node.kind, NodeKind::Text);
            assert!(matches!(node.content, Some(ContentConcern::Text(ref text))
                if text.font == TextStyleRef::icon() && !text.text.is_empty()));
        }
    }

    #[test]
    fn provider_rejects_unknown_semantic_key() {
        let result = MaterialIconFontProvider.resolve(IconRequest {
            key: IconKey::from("not-in-material"),
            size: 20.0,
            color: Color::WHITE,
        });

        assert!(result.is_err());
    }
}
