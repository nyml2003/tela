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
        // The rounded font's `maximize` codepoint is rendered as a horizontal
        // rule at the fixed axes used by this subset. A rounded square is the
        // intended native window maximize control.
        IconName::Maximize => '\u{e3c6}',
        IconName::WindowRestore => '\u{e3e0}',
        IconName::Redo => '\u{e15a}',
        IconName::Cut => '\u{f08b}',
        IconName::Paste => '\u{e14f}',
        IconName::Save => '\u{e161}',
        IconName::SaveAs => '\u{eb60}',
        IconName::SelectAll => '\u{e162}',
        IconName::FindReplace => '\u{e881}',
        IconName::FormatBold => '\u{e238}',
        IconName::FormatItalic => '\u{e23f}',
        IconName::FormatUnderlined => '\u{e249}',
        IconName::FormatAlignLeft => '\u{e236}',
        IconName::FormatAlignCenter => '\u{e234}',
        IconName::FormatAlignRight => '\u{e237}',
        IconName::FormatSize => '\u{e245}',
        IconName::Spellcheck => '\u{e8ce}',
        IconName::Remove => '\u{e15b}',
        IconName::RemoveCircle => '\u{f08f}',
        IconName::DeleteForever => '\u{e92b}',
        IconName::FileCopy => '\u{e173}',
        IconName::Article => '\u{ef87}',
        IconName::Draft => '\u{e674}',
        IconName::PictureAsPdf => '\u{e415}',
        IconName::CreateNewFolder => '\u{e2cc}',
        IconName::AttachFile => '\u{e226}',
        IconName::Link => '\u{e250}',
        IconName::LinkOff => '\u{e16f}',
        IconName::Download => '\u{f090}',
        IconName::Upload => '\u{f09b}',
        IconName::Cloud => '\u{f15c}',
        IconName::CloudDownload => '\u{e2c0}',
        IconName::CloudUpload => '\u{e2c3}',
        IconName::DriveFileMove => '\u{e9a1}',
        IconName::FolderZip => '\u{eb2c}',
        IconName::Unarchive => '\u{e169}',
        IconName::Print => '\u{e8ad}',
        IconName::ArrowForward => '\u{e5c8}',
        IconName::ArrowUpward => '\u{e5d8}',
        IconName::ArrowDownward => '\u{e5db}',
        IconName::ChevronLeft => '\u{e5cb}',
        IconName::ExpandLess => '\u{e5ce}',
        IconName::ExpandMore => '\u{e5cf}',
        IconName::Fullscreen => '\u{e5d0}',
        IconName::FullscreenExit => '\u{e5d1}',
        IconName::OpenInNew => '\u{e89e}',
        IconName::Launch => '\u{e89e}',
        IconName::Home => '\u{e9b2}',
        IconName::MenuOpen => '\u{e9bd}',
        IconName::Check => '\u{e668}',
        IconName::CheckCircle => '\u{f0be}',
        IconName::Cancel => '\u{e888}',
        IconName::Error => '\u{f8b6}',
        IconName::Warning => '\u{f083}',
        IconName::Info => '\u{e88e}',
        IconName::Help => '\u{e8fd}',
        IconName::Verified => '\u{ef76}',
        IconName::Lock => '\u{e899}',
        IconName::LockOpen => '\u{e898}',
        IconName::Visibility => '\u{e8f4}',
        IconName::VisibilityOff => '\u{e8f5}',
        IconName::Refresh => '\u{e5d5}',
        IconName::Sync => '\u{e627}',
        IconName::History => '\u{e8b3}',
        IconName::ViewList => '\u{e8ef}',
        IconName::ViewModule => '\u{e8f0}',
        IconName::ViewQuilt => '\u{e8f1}',
        IconName::GridView => '\u{e9b0}',
        IconName::FilterAlt => '\u{ef4f}',
        IconName::FilterAltOff => '\u{eb32}',
        IconName::Tune => '\u{e429}',
        IconName::TableChart => '\u{e265}',
        IconName::ZoomIn => '\u{e8ff}',
        IconName::ZoomOut => '\u{e900}',
        IconName::Person => '\u{f0d3}',
        IconName::People => '\u{ea21}',
        IconName::Group => '\u{ea21}',
        IconName::AccountCircle => '\u{f20b}',
        IconName::Mail => '\u{e159}',
        IconName::Chat => '\u{e0c9}',
        IconName::Comment => '\u{e24c}',
        IconName::Share => '\u{e80d}',
        IconName::Notifications => '\u{e7f5}',
        IconName::PlayArrow => '\u{e037}',
        IconName::Pause => '\u{e034}',
        IconName::Stop => '\u{e047}',
        IconName::SkipNext => '\u{e044}',
        IconName::SkipPrevious => '\u{e045}',
        IconName::VolumeUp => '\u{e050}',
        IconName::VolumeOff => '\u{e04f}',
        IconName::Mic => '\u{e31d}',
        IconName::Movie => '\u{e684}',
        IconName::CameraAlt => '\u{e412}',
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
        assert_eq!(IconName::ALL.len(), 120);
        let keys: std::collections::HashSet<_> =
            IconName::ALL.iter().map(|name| name.key()).collect();
        assert_eq!(keys.len(), IconName::ALL.len(), "icon keys must be unique");
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

    #[test]
    fn window_controls_use_the_material_window_control_codepoints() {
        assert_eq!(super::material_codepoint(IconName::Minimize), '\u{e931}');
        assert_eq!(super::material_codepoint(IconName::Maximize), '\u{e3c6}');
        assert_eq!(
            super::material_codepoint(IconName::WindowRestore),
            '\u{e3e0}'
        );
        assert_eq!(super::material_codepoint(IconName::Close), '\u{e5cd}');
    }
}
