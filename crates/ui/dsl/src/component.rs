//! 组件契约与内建原语适配层（033 方案）。
//!
//! DSL 只有一个概念——"组件"：所有 UI 元素（Row/Column/Text/自定义组件）实现
//! [`DslComponent`]，标签可见性由 Rust `use` 决定，宏只做属性搬运工。

use tela_contract::{
    BorderRadius, Color, ContentConcern, CrossAlign, Fill, HitRole, Insets, InteractConcern,
    KeyboardInputSpec, KeymapScopeId, LayoutConcern, NodeKind, Overflow, OverlaySpec, PixelOffset,
    ShadowSpec, ShortcutScopeSpec, Size, StackAlign, TextContent, TextInputSpec, TextStyleRef,
    UiNode, UpdateMode, VisualConcern,
};

use crate::{
    Body, Children, ComponentIdentity, ComponentOutcome, ComponentRenderContext,
    ComponentSetupContext, TransitionExt, TransitionSpec, ViewBuild, ViewBuildError, ViewChild,
    ViewOutput, ViewResult, ViewSite,
};

/// 声明式组件的统一 setup/render/handler 生命周期契约。
///
/// Props 字段构成标签属性；State 由 DSL 候选帧保存；render 只能读取 State；handler 是
/// 修改私有 State 和产生类型化 Output 的唯一入口。children 保持惰性，父组件可以先建立
/// provide 作用域，再决定是否展开子树。
pub trait DslComponent {
    /// 组件 Props。约定字段一律为 `Option<T>`（未提供走 `Default` 的 `None`）。
    type Props: Default;
    /// 组件私有跨帧状态。
    type State: Clone + Default + 'static;
    /// 组件内部事件。
    type Event;
    /// 允许离开组件边界的语义输出。
    type Output;

    /// 从 Props 提取显式实例 key。默认组件没有显式 key。
    fn identity_key(_props: &Self::Props) -> Option<String> {
        None
    }

    /// 首次建立该组件身份时初始化 State。
    fn setup(_context: &ComponentSetupContext, _props: &Self::Props) -> Self::State {
        Self::State::default()
    }

    /// 用只读 State、Props 和惰性 children 构建候选子树。
    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>>;

    /// 在候选状态上处理本地事件。
    fn handle(
        _state: &mut Self::State,
        _props: &Self::Props,
        _event: Self::Event,
    ) -> ComponentOutcome<Self::Output> {
        ComponentOutcome::Consumed
    }

    /// 把组件声明的本地事件路由附着到候选视图，并静态映射类型化 Output。
    ///
    /// 具有交互 Output 的组件覆盖此方法；纯展示组件若被错误地声明 `output={...}`，会
    /// 得到结构化构建错误。映射只能是函数项，不能捕获组件 State 或 Host 对象。
    fn bind_output<A: 'static>(
        view: ViewOutput<A>,
        _identity: ComponentIdentity,
        _props: &Self::Props,
        _output: fn(Self::Output) -> Option<A>,
        site: ViewSite,
    ) -> ViewResult<ViewOutput<A>>
    where
        Self: Sized + 'static,
        Self::Props: Clone + 'static,
    {
        let _ = view;
        Err(ViewBuildError::UnsupportedComponentOutput {
            component: std::any::type_name::<Self>(),
            site,
        })
    }
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
        if let Some(value) = $props.border_radii {
            __visual.border_radius = value;
        }
        if let Some(value) = $props.shadow {
            __visual.shadow = Some(value);
        }
        if let Some(value) = $props.opacity {
            __visual.opacity = value.clamp(0.0, 1.0);
        }
        if let Some(value) = $props.visual_offset {
            __visual.visual_offset = value;
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
        if $props.window_drag_region.unwrap_or(false) {
            __interact.hit_role = HitRole::WindowDrag;
        }
        if let Some(value) = $props.input {
            __interact.input = Some(value);
        }
        if let Some(value) = $props.keyboard {
            __interact.keyboard = Some(value);
        }
        if __interact != InteractConcern::default() {
            $node.interact = Some(__interact);
        }
        if let Some(value) = $props.update_mode {
            $node
                .identity
                .get_or_insert_with(Default::default)
                .update_mode = value;
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
            pub border_radii: Option<BorderRadius>,
            pub shadow: Option<ShadowSpec>,
            pub opacity: Option<f32>,
            pub visual_offset: Option<PixelOffset>,
            pub transition: Option<TransitionSpec>,
            pub update_mode: Option<UpdateMode>,
            pub clickable: Option<bool>,
            pub hoverable: Option<bool>,
            pub focusable: Option<bool>,
            pub window_drag_region: Option<bool>,
            pub input: Option<TextInputSpec>,
            pub keyboard: Option<KeyboardInputSpec>,
        }

        impl DslComponent for $name {
            type Props = $name;
            type State = ();
            type Event = ();
            type Output = ();

            fn identity_key(props: &Self::Props) -> Option<String> {
                props.key.clone()
            }

            fn render<'a, A>(
                context: &mut ComponentRenderContext<'_, A>,
                props: Self::Props,
                _state: &Self::State,
                children: Children<'a, A>,
            ) -> ViewResult<ViewOutput<A>> {
                let site = context.site();
                let children = children.build(context.build())?;
                if $check_single_child && children.child_count() != 1 {
                    return Err(ViewBuildError::ExpectedSingleRoot {
                        actual: children.child_count(),
                        site,
                    });
                }
                let mut node = UiNode::new($kind);
                apply_primitive_fields!(node, props);
                if let Some(transition) = props.transition {
                    let target = node.visual.clone().unwrap_or_default();
                    node.visual = Some(
                        context
                            .transition(
                                "visual",
                                target.transition(transition.duration_ms, transition.easing),
                            )
                            .value,
                    );
                }
                let build = context.build();
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

/// `<Overlay>` 的 Stack 锚定参数。
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(missing_docs)]
pub struct Overlay {
    pub align: Option<StackAlign>,
    pub offset: Option<PixelOffset>,
    pub fill_width: Option<bool>,
    pub fill_height: Option<bool>,
    pub modal: Option<bool>,
}

impl DslComponent for Overlay {
    type Props = Overlay;
    type State = ();
    type Event = ();
    type Output = ();

    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let build = context.build();
        let children = children.build(build)?;
        if children.child_count() != 1 {
            return Err(ViewBuildError::ExpectedSingleRoot {
                actual: children.child_count(),
                site,
            });
        }
        let mut node = UiNode::new(NodeKind::Overlay(OverlaySpec {
            align: props.align.unwrap_or_default(),
            offset: props.offset.unwrap_or_default(),
            fill_width: props.fill_width.unwrap_or(false),
            fill_height: props.fill_height.unwrap_or(false),
        }));
        if props.modal.unwrap_or(false) {
            node.interact = Some(InteractConcern {
                modal: true,
                ..InteractConcern::default()
            });
        }
        let node = build.container(node, children)?;
        finish_node(build, node, site)
    }
}

/// `<ShortcutScope>` 的局部键位表作用域参数。
#[derive(Clone, Debug, Default, PartialEq)]
#[allow(missing_docs)]
pub struct ShortcutScope {
    pub id: Option<KeymapScopeId>,
}

impl DslComponent for ShortcutScope {
    type Props = ShortcutScope;
    type State = ();
    type Event = ();
    type Output = ();

    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let build = context.build();
        let children = children.build(build)?;
        let id = props
            .id
            .ok_or(ViewBuildError::MissingRequiredProp { name: "id", site })?;
        let node = UiNode::new(NodeKind::ShortcutScope(ShortcutScopeSpec { id }));
        let node = build.container(node, children)?;
        finish_node(build, node, site)
    }
}

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
            pub border_radii: Option<BorderRadius>,
            pub shadow: Option<ShadowSpec>,
            pub opacity: Option<f32>,
            pub visual_offset: Option<PixelOffset>,
            pub transition: Option<TransitionSpec>,
            pub update_mode: Option<UpdateMode>,
            pub clickable: Option<bool>,
            pub hoverable: Option<bool>,
            pub focusable: Option<bool>,
            pub window_drag_region: Option<bool>,
            pub input: Option<TextInputSpec>,
            pub keyboard: Option<KeyboardInputSpec>,
            pub value: Option<String>,
            pub font: Option<TextStyleRef>,
            pub font_size: Option<f32>,
            pub line_height: Option<f32>,
            pub color: Option<Color>,
        }

        impl DslComponent for $name {
            type Props = $name;
            type State = ();
            type Event = ();
            type Output = ();

            fn identity_key(props: &Self::Props) -> Option<String> {
                props.key.clone()
            }

            fn render<'a, A>(
                context: &mut ComponentRenderContext<'_, A>,
                props: Self::Props,
                _state: &Self::State,
                children: Children<'a, A>,
            ) -> ViewResult<ViewOutput<A>> {
                let site = context.site();
                let _children = children.build(context.build())?;
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
                if let Some(transition) = props.transition {
                    let target = node.visual.clone().unwrap_or_default();
                    node.visual = Some(
                        context
                            .transition(
                                "visual",
                                target.transition(transition.duration_ms, transition.easing),
                            )
                            .value,
                    );
                }
                let build = context.build();
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
    pub border_radii: Option<BorderRadius>,
    pub shadow: Option<ShadowSpec>,
    pub opacity: Option<f32>,
    pub visual_offset: Option<PixelOffset>,
    pub transition: Option<TransitionSpec>,
    pub update_mode: Option<UpdateMode>,
    pub clickable: Option<bool>,
    pub hoverable: Option<bool>,
    pub focusable: Option<bool>,
    pub window_drag_region: Option<bool>,
    pub input: Option<TextInputSpec>,
    pub keyboard: Option<KeyboardInputSpec>,
    pub texture: Option<String>,
}

impl DslComponent for Image {
    type Props = Image;
    type State = ();
    type Event = ();
    type Output = ();

    fn identity_key(props: &Self::Props) -> Option<String> {
        props.key.clone()
    }

    fn render<'a, A>(
        context: &mut ComponentRenderContext<'_, A>,
        props: Self::Props,
        _state: &Self::State,
        children: Children<'a, A>,
    ) -> ViewResult<ViewOutput<A>> {
        let site = context.site();
        let _children = children.build(context.build())?;
        let mut node = UiNode::new(NodeKind::Image).with_content(ContentConcern::Image(
            tela_contract::ImageContent {
                texture: tela_contract::TextureRef(props.texture.unwrap_or_default()),
            },
        ));
        apply_primitive_fields!(node, props);
        if let Some(transition) = props.transition {
            let target = node.visual.clone().unwrap_or_default();
            node.visual = Some(
                context
                    .transition(
                        "visual",
                        target.transition(transition.duration_ms, transition.easing),
                    )
                    .value,
            );
        }
        let build = context.build();
        let view_node = build.container(node, Body::new(Vec::new(), Vec::new()))?;
        let view_node = apply_key(view_node, props.key);
        finish_node(build, view_node, site)
    }
}

/// DSL 组件 prelude：一次性引入全部内建原语组件与契约。
pub mod prelude {
    pub use super::{
        Column, DslComponent, Frame, Icon, Image, Overlay, Row, ScrollView, ShortcutScope, Stack,
        Text, View,
    };
}
