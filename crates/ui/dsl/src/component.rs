//! 组件契约与内建原语适配层（033 方案）。
//!
//! DSL 只有一个概念——"组件"：所有 UI 元素（Row/Column/Text/自定义组件）实现
//! [`DslComponent`]，标签可见性由 Rust `use` 决定，宏只做属性搬运工。

use tela_contract::{
    BindId, BorderRadius, Color, ContentConcern, CrossAlign, Fill, Insets, InteractConcern,
    LayoutConcern, NodeKind, Overflow, Size, TextContent, TextInputSpec, TextStyleRef, UiNode,
    VisualConcern,
};

use crate::{Body, ViewBuild, ViewBuildError, ViewChild, ViewOutput, ViewResult, ViewSite};

/// 组件契约：`type Props` 的字段名即标签属性名（snake_case），`render` 负责构建子树。
///
/// `render` 是泛型方法（动作类型 `A` 由 `ViewBuild<A>` 推断），因此组件实现不绑定
/// 具体动作类型；`type Props` 也因此不依赖 `A`，调用点可以按名称构造 Props。
/// `children` 以 [`Body`] 传入，容器组件直接交给 `build.container`，watch 计划不会丢失。
pub trait DslComponent {
    /// 组件 Props。约定字段一律为 `Option<T>`（未提供走 `Default` 的 `None`）。
    type Props: Default;

    /// 用已解析的 props 与子节点构建本组件。
    fn render<A>(
        build: &mut ViewBuild<A>,
        props: Self::Props,
        children: Body<A>,
    ) -> ViewResult<ViewOutput<A>>;
}

/// 把公共 Props 字段应用到原语节点（布局/视觉/交互三段式）。
macro_rules! apply_primitive_fields {
    ($node:ident, $props:ident) => {{
        let mut __layout = LayoutConcern::default();
        if let Some(value) = $props.width {
            __layout.width = Some(Size::fixed(value));
        }
        if let Some(value) = $props.height {
            __layout.height = Some(Size::fixed(value));
        }
        if let Some(value) = $props.margin {
            __layout.margin = value;
        }
        if let Some(value) = $props.padding {
            __layout.padding = value;
        }
        if let Some(value) = $props.border_width {
            __layout.border_width = value;
        }
        if let Some(value) = $props.gap {
            __layout.gap = value;
        }
        if let Some(value) = $props.cross_align {
            __layout.cross_align = value;
        }
        if let Some(value) = $props.clip {
            __layout.clip = value;
        }
        if let Some(value) = $props.overflow {
            __layout.overflow = value;
        }
        if __layout != LayoutConcern::default() {
            $node.layout = Some(__layout);
        }
        let mut __visual = VisualConcern::default();
        if let Some(value) = $props.fill {
            __visual.fill = Some(value);
        }
        if let Some(value) = $props.border_color {
            __visual.border_color = Some(value);
        }
        if let Some(value) = $props.border_radius {
            __visual.border_radius = BorderRadius::all(value);
        }
        if __visual != VisualConcern::default() {
            $node.visual = Some(__visual);
        }
        let mut __interact = InteractConcern::default();
        if let Some(value) = $props.clickable {
            __interact.clickable = value;
        }
        if let Some(value) = $props.hoverable {
            __interact.hoverable = value;
        }
        if let Some(value) = $props.focusable {
            __interact.focusable = value;
        }
        if let Some(value) = $props.input {
            __interact.input = Some(value);
        }
        if let Some(value) = $props.bind_id {
            __interact.bind_id = Some(BindId(value));
        }
        if __interact != InteractConcern::default() {
            $node.interact = Some(__interact);
        }
    }};
}

/// 把 `key` 应用到构建好的 ViewNode 上（约定字段 `key: Option<String>`）。
fn apply_key<A>(node: crate::ViewNode<A>, key: Option<String>) -> crate::ViewNode<A> {
    match key {
        Some(key) => node.with_semantic_key(key),
        None => node,
    }
}

/// 将 ViewNode 包装为单根 ViewOutput（保留帧期计划）。
fn finish_node<A>(
    build: &mut ViewBuild<A>,
    node: crate::ViewNode<A>,
    site: ViewSite,
) -> ViewResult<ViewOutput<A>> {
    build.finish(
        Body::new(vec![ViewChild::view_node(node)], Vec::new()),
        site,
    )
}

macro_rules! primitive_component {
    ($name:ident, $kind:expr, $check_single_child:expr) => {
        /// 原语容器组件（见 033：与自定义组件地位平等）。
        #[derive(Clone, Debug, Default, PartialEq)]
        #[doc = concat!("`<", stringify!($name), " ...>` 的 Props。")]
        #[allow(missing_docs)]
        pub struct $name {
            pub key: Option<String>,
            pub width: Option<f32>,
            pub height: Option<f32>,
            pub margin: Option<Insets>,
            pub padding: Option<Insets>,
            pub border_width: Option<f32>,
            pub gap: Option<f32>,
            pub cross_align: Option<CrossAlign>,
            pub clip: Option<bool>,
            pub overflow: Option<Overflow>,
            pub fill: Option<Fill>,
            pub border_color: Option<Color>,
            pub border_radius: Option<f32>,
            pub clickable: Option<bool>,
            pub hoverable: Option<bool>,
            pub focusable: Option<bool>,
            pub input: Option<TextInputSpec>,
            pub bind_id: Option<String>,
        }

        impl DslComponent for $name {
            type Props = $name;

            fn render<A>(
                build: &mut ViewBuild<A>,
                props: Self::Props,
                children: Body<A>,
            ) -> ViewResult<ViewOutput<A>> {
                let site = ViewSite::new(file!(), line!(), column!());
                if $check_single_child && children.child_count() != 1 {
                    return Err(ViewBuildError::ExpectedSingleRoot {
                        actual: children.child_count(),
                        site,
                    });
                }
                let mut node = UiNode::new($kind);
                apply_primitive_fields!(node, props);
                let view_node = build.container(node, children)?;
                let view_node = apply_key(view_node, props.key);
                finish_node(build, view_node, site)
            }
        }
    };
}

primitive_component!(Row, NodeKind::Row, false);
primitive_component!(Column, NodeKind::Column, false);
primitive_component!(Frame, NodeKind::Frame, true);
primitive_component!(View, NodeKind::View, false);
primitive_component!(Stack, NodeKind::Stack, false);
primitive_component!(ScrollView, NodeKind::ScrollView, false);

macro_rules! text_component {
    ($name:ident, $icon:expr) => {
        /// 文本原语组件（见 033）。
        #[derive(Clone, Debug, Default, PartialEq)]
        #[doc = concat!("`<", stringify!($name), " ...>` 的 Props。")]
        #[allow(missing_docs)]
        pub struct $name {
            pub key: Option<String>,
            pub width: Option<f32>,
            pub height: Option<f32>,
            pub margin: Option<Insets>,
            pub padding: Option<Insets>,
            pub border_width: Option<f32>,
            pub gap: Option<f32>,
            pub cross_align: Option<CrossAlign>,
            pub clip: Option<bool>,
            pub overflow: Option<Overflow>,
            pub fill: Option<Fill>,
            pub border_color: Option<Color>,
            pub border_radius: Option<f32>,
            pub clickable: Option<bool>,
            pub hoverable: Option<bool>,
            pub focusable: Option<bool>,
            pub input: Option<TextInputSpec>,
            pub bind_id: Option<String>,
            pub value: Option<String>,
            pub font: Option<TextStyleRef>,
            pub font_size: Option<f32>,
            pub line_height: Option<f32>,
            pub color: Option<Color>,
        }

        impl DslComponent for $name {
            type Props = $name;

            fn render<A>(
                build: &mut ViewBuild<A>,
                props: Self::Props,
                _children: Body<A>,
            ) -> ViewResult<ViewOutput<A>> {
                let site = ViewSite::new(file!(), line!(), column!());
                let mut node =
                    UiNode::new(NodeKind::Text).with_content(ContentConcern::Text(TextContent {
                        text: props.value.unwrap_or_default(),
                        font: props.font.unwrap_or_else(|| {
                            if $icon {
                                TextStyleRef::icon()
                            } else {
                                TextStyleRef::body()
                            }
                        }),
                        font_size: props.font_size.unwrap_or(14.0),
                        line_height: props.line_height.unwrap_or(20.0),
                        color: props.color.unwrap_or(Color::BLACK),
                    }));
                apply_primitive_fields!(node, props);
                let view_node = build.container(node, Body::new(Vec::new(), Vec::new()))?;
                let view_node = apply_key(view_node, props.key);
                finish_node(build, view_node, site)
            }
        }
    };
}

text_component!(Text, false);
text_component!(Icon, true);

/// `<Image>` 原语组件。
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(missing_docs)]
pub struct Image {
    pub key: Option<String>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub margin: Option<Insets>,
    pub padding: Option<Insets>,
    pub border_width: Option<f32>,
    pub gap: Option<f32>,
    pub cross_align: Option<CrossAlign>,
    pub clip: Option<bool>,
    pub overflow: Option<Overflow>,
    pub fill: Option<Fill>,
    pub border_color: Option<Color>,
    pub border_radius: Option<f32>,
    pub clickable: Option<bool>,
    pub hoverable: Option<bool>,
    pub focusable: Option<bool>,
    pub input: Option<TextInputSpec>,
    pub bind_id: Option<String>,
    pub texture: Option<String>,
}

impl DslComponent for Image {
    type Props = Image;

    fn render<A>(
        build: &mut ViewBuild<A>,
        props: Self::Props,
        _children: Body<A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = ViewSite::new(file!(), line!(), column!());
        let mut node = UiNode::new(NodeKind::Image).with_content(ContentConcern::Image(
            tela_contract::ImageContent {
                texture: tela_contract::TextureRef(props.texture.unwrap_or_default()),
            },
        ));
        apply_primitive_fields!(node, props);
        let view_node = build.container(node, Body::new(Vec::new(), Vec::new()))?;
        let view_node = apply_key(view_node, props.key);
        finish_node(build, view_node, site)
    }
}

/// DSL 组件 prelude：一次性引入全部内建原语组件与契约。
pub mod prelude {
    pub use super::{Column, DslComponent, Frame, Icon, Image, Row, ScrollView, Stack, Text, View};
}
