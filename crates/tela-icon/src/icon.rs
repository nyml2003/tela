//! 面向组件调用方的图标原子组件。

use tela_contract::{Color, UiNode};

use crate::{
    IconKey, IconName, IconProvider, IconRequest, IconResolveError, IconVisual,
    material::resolve_material_icon,
};

/// 使用默认 Material provider 的图标原子组件。
pub struct Icon {
    name: IconName,
    size: f32,
    color: Color,
}

impl Icon {
    /// 用内建语义图标创建图标组件。
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: 18.0,
            color: Color::rgba(0.12, 0.18, 0.28, 1.0),
        }
    }

    /// 设置图标字号与逻辑盒尺寸。
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
    pub fn key(&self) -> IconKey {
        self.name.into()
    }

    /// 通过指定 provider 解析图标视觉输出。
    pub fn resolve_with(
        self,
        provider: &impl IconProvider,
    ) -> Result<IconVisual, IconResolveError> {
        provider.resolve(IconRequest {
            key: self.name.into(),
            size: self.size,
            color: self.color,
        })
    }

    /// 返回内建 Material provider 的未补偿视觉输出。
    ///
    /// 普通调用方应直接使用 [`Self::into_node`]；需要与相邻文本精确 optical 对齐的分子
    /// 组件可以消费这份输出，并以 provider 报告的实际墨迹度量选择目标中心。
    pub fn into_visual(self) -> IconVisual {
        resolve_material_icon(self.name, self.size, self.color)
    }

    /// 生成使用内建 Material iconfont 的图标节点。
    pub fn into_node(self) -> UiNode {
        self.into_visual().into_node()
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{ContentConcern, FontRef};

    use crate::{Icon, IconName};

    #[test]
    fn default_icon_applies_the_provider_measured_visual_offset() {
        let visual = Icon::new(IconName::Folder).size(20.0).into_visual();
        let expected_offset = visual.metrics().center_offset_y();
        let node = visual.into_node();
        let text = match node.content {
            Some(ContentConcern::Text(text)) => text,
            other => panic!("expected icon text node, got {other:?}"),
        };
        assert_eq!(text.font, FontRef(tela_fonts::ICON_FONT_NAME.to_owned()));
        let offset = node.visual.expect("icon visual concern").visual_offset;
        assert!(
            (offset.y - expected_offset).abs() < f32::EPSILON,
            "optical correction must come from provider metrics"
        );
    }
}
