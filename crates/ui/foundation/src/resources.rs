//! UI 侧的图标原子请求构建器。
//!
//! 图标语义、provider 和产品资源注入协议属于 `tela-contract`：它们是所有责任域交换的
//! 纯值。Foundation 只保留把语义键组合成受控 UI 节点的原子，不选择具体视觉资源。

use tela_contract::{Color, IconKey, IconProvider, IconRequest, IconResolveError, IconVisual};

/// 图标原子请求构建器。
///
/// 它只保存语义请求，必须通过产品注入的 [`IconProvider`] 才能转为节点，因此调用方
/// 不会隐式选择 Material、字体文件或平台 fallback。
pub struct Icon {
    key: IconKey,
    size: f32,
    color: Color,
}

impl Icon {
    /// 用一个语义键创建图标请求。
    pub fn new(key: impl Into<IconKey>) -> Self {
        Self {
            key: key.into(),
            size: 18.0,
            color: Color::rgba(0.12, 0.18, 0.28, 1.0),
        }
    }

    /// 设置图标逻辑盒尺寸。
    pub fn size(mut self, size: f32) -> Self {
        self.size = size.max(1.0);
        self
    }

    /// 设置图标颜色。
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// 返回来源无关的图标语义键。
    pub fn key(&self) -> &IconKey {
        &self.key
    }

    /// 通过产品提供的资源解析图标视觉输出。
    pub fn resolve_with(self, provider: &dyn IconProvider) -> Result<IconVisual, IconResolveError> {
        provider.resolve(IconRequest {
            key: self.key,
            size: self.size,
            color: self.color,
        })
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{
        Color, IconName, IconOpticalMetrics, IconProvider, IconRequest, IconVisual, NodeKind,
        TextContent,
    };

    use super::Icon;

    struct TestProvider;

    impl IconProvider for TestProvider {
        fn resolve(
            &self,
            request: IconRequest,
        ) -> Result<IconVisual, tela_contract::IconResolveError> {
            Ok(IconVisual::new(
                tela_core::Primitive::text(TextContent {
                    text: request.key.as_str().to_owned(),
                    font: tela_contract::TextStyleRef::icon(),
                    font_size: request.size,
                    line_height: request.size,
                    color: request.color,
                })
                .into(),
                IconOpticalMetrics {
                    box_size: request.size,
                    ink_center_y: request.size * 0.25,
                },
            ))
        }
    }

    #[test]
    fn icon_keeps_the_semantic_key_until_a_provider_resolves_it() {
        let icon = Icon::new(IconName::Folder).size(20.0).color(Color::WHITE);
        assert_eq!(icon.key().as_str(), "folder");

        let node = icon
            .resolve_with(&TestProvider)
            .expect("test provider")
            .into_node();
        assert_eq!(node.kind, NodeKind::Text);
        assert_eq!(node.visual.expect("visual offset").visual_offset.y, 5.0);
    }
}
