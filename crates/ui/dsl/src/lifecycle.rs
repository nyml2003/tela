//! 声明式组件的 setup/render/handler 生命周期协议。
//!
//! 状态由 `ViewBuild` 的候选 owner 帧保存，应用不需要持有 owner 或身份对象。

use std::sync::Arc;

use crate::{
    Children, DslComponent, ViewBuild, ViewContext, ViewOutput, ViewResult, ViewSite,
    owner::ComponentState,
};

/// setup 阶段可读取的只读上下文。
pub struct ComponentSetupContext {
    scope: Arc<ViewContext>,
}

impl ComponentSetupContext {
    pub(crate) fn new(scope: Arc<ViewContext>) -> Self {
        Self { scope }
    }

    /// 当前 provide/inject 作用域快照。
    pub fn scope(&self) -> &ViewContext {
        &self.scope
    }

    /// 从当前作用域读取已提供值。
    pub fn inject<T: Send + Sync + Clone + 'static>(&self, site: ViewSite) -> Option<T> {
        self.scope.inject::<T>(site).ok().cloned()
    }
}

/// render 阶段上下文；只允许通过 `ViewBuild` 生成节点和附属计划。
pub struct ComponentRenderContext<'a, A> {
    build: &'a mut ViewBuild<A>,
    site: ViewSite,
}

impl<'a, A> ComponentRenderContext<'a, A> {
    pub(crate) fn new(build: &'a mut ViewBuild<A>, site: ViewSite) -> Self {
        Self { build, site }
    }

    /// 当前组件调用点。
    pub fn site(&self) -> ViewSite {
        self.site
    }

    /// 访问底层 ViewBuild。组件只能在 render 中用它构造候选节点。
    pub fn build(&mut self) -> &mut ViewBuild<A> {
        self.build
    }
}

/// handler 阶段的处理结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComponentOutcome<O> {
    /// 事件与组件匹配，但没有改变私有状态，也没有产生输出。
    Ignored,
    /// 事件只改变内部状态，不产生应用输出。
    Consumed,
    /// 事件产生一个提交后才会外传的语义结果。
    Output(O),
}

/// 按 DSL 调用点取得组件私有状态，并执行生命周期 render。
pub fn render_component<'a, C, A>(
    build: &mut ViewBuild<A>,
    props: C::Props,
    children: Children<'a, A>,
    site: ViewSite,
) -> ViewResult<ViewOutput<A>>
where
    C: DslComponent,
{
    let key = C::identity_key(&props);
    let identity = build.component_identity(std::any::type_name::<C>(), site, key);
    let setup_scope = build.current_scope();
    let state: ComponentState<C::State> = build.local_state_for(identity.clone(), || {
        C::setup(&ComponentSetupContext::new(setup_scope), &props)
    });
    let output = build.with_component_identity(&identity, |build| {
        let mut context = ComponentRenderContext::new(build, site);
        C::render(&mut context, props, &state.get(), children)
    })?;
    // A component may intentionally return an opaque or kit-provided node. The owner frame is
    // still part of the component's render result; otherwise state materialized by setup/render
    // is dropped before FrameCoordinator can commit it and later input cannot dispatch.
    Ok(output.with_owner_frame(
        build
            .owner_frame
            .clone()
            .expect("component render must retain the candidate owner frame"),
    ))
}

/// 渲染组件并附着其静态 `Output -> AppAction` 映射。
///
/// `ui!` 的 `output={function_path}` 统一 lowering 到此入口；组件身份只由当前 DSL
/// 调用点计算，应用既不创建 identity，也不接触本地 Event。
pub fn render_component_with_output<'a, C, A>(
    build: &mut ViewBuild<A>,
    props: C::Props,
    children: Children<'a, A>,
    output: fn(C::Output) -> Option<A>,
    site: ViewSite,
) -> ViewResult<ViewOutput<A>>
where
    C: DslComponent + 'static,
    C::Props: Clone + 'static,
    A: 'static,
{
    let identity =
        build.component_identity(std::any::type_name::<C>(), site, C::identity_key(&props));
    let route_props = props.clone();
    let view = render_component::<C, A>(build, props, children, site)?;
    C::bind_output(view, identity, &route_props, output, site)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tela_contract::{
        ContentConcern, NodeKind, TextContent, TextStyleRef, UiFrame, UiNode, Viewport,
    };

    use super::{
        ComponentOutcome, ComponentRenderContext, ComponentSetupContext, render_component,
    };
    use crate::component::Column;
    use crate::{
        Body, Children, FrameCoordinator, ViewBuild, ViewChild, ViewOutput, ViewResult, ViewSite,
        ui,
    };

    static SETUPS: AtomicUsize = AtomicUsize::new(0);

    struct Stateful;

    impl crate::DslComponent for Stateful {
        type Props = ();
        type State = usize;
        type Event = ();
        type Output = ();

        fn setup(_context: &ComponentSetupContext, _props: &Self::Props) -> Self::State {
            SETUPS.fetch_add(1, Ordering::SeqCst);
            7
        }

        fn render<'a, A>(
            context: &mut ComponentRenderContext<'_, A>,
            _props: Self::Props,
            state: &Self::State,
            _children: Children<'a, A>,
        ) -> ViewResult<ViewOutput<A>> {
            assert_eq!(*state, 7);
            let site = context.site();
            context.build().finish(
                Body::new(
                    vec![ViewChild::node(UiNode::new(NodeKind::View))],
                    Vec::new(),
                ),
                site,
            )
        }

        fn handle(
            _state: &mut Self::State,
            _props: &Self::Props,
            _event: Self::Event,
        ) -> ComponentOutcome<Self::Output> {
            ComponentOutcome::Consumed
        }
    }

    fn publish(coordinator: &mut FrameCoordinator<()>) {
        let mut build = coordinator.begin_build();
        let root = render_component::<Stateful, _>(
            &mut build,
            (),
            Children::new(|_| Ok(Body::new(Vec::new(), Vec::new()))),
            ViewSite::new("stateful", 1, 1),
        )
        .expect("stateful view");
        let prepared = coordinator.prepare(root).expect("prepared");
        let resolved = prepared
            .resolve(|_| {
                Ok::<_, ()>(UiFrame {
                    viewport: Viewport {
                        width: 1.0,
                        height: 1.0,
                    },
                    commands: Vec::new(),
                    hit_regions: Vec::new(),
                    scroll_bounds: Vec::new(),
                })
            })
            .expect("resolved");
        coordinator.commit(resolved);
    }

    fn publish_plain(coordinator: &mut FrameCoordinator<()>) {
        let mut build = coordinator.begin_build();
        let root = build
            .finish(
                Body::new(
                    vec![ViewChild::node(UiNode::new(NodeKind::View))],
                    Vec::new(),
                ),
                ViewSite::new("plain", 1, 1),
            )
            .expect("plain view");
        let resolved = coordinator
            .prepare(root)
            .expect("prepared")
            .resolve(|_| {
                Ok::<_, ()>(UiFrame {
                    viewport: Viewport {
                        width: 1.0,
                        height: 1.0,
                    },
                    commands: Vec::new(),
                    hit_regions: Vec::new(),
                    scroll_bounds: Vec::new(),
                })
            })
            .expect("resolved");
        coordinator.commit(resolved);
    }

    struct OpaqueStateful;

    impl crate::DslComponent for OpaqueStateful {
        type Props = ();
        type State = u8;
        type Event = ();
        type Output = ();

        fn setup(_context: &ComponentSetupContext, _props: &Self::Props) -> Self::State {
            1
        }

        fn render<'a, A>(
            _context: &mut ComponentRenderContext<'_, A>,
            _props: Self::Props,
            _state: &Self::State,
            _children: Children<'a, A>,
        ) -> ViewResult<ViewOutput<A>> {
            Ok(ViewOutput::opaque(UiNode::new(NodeKind::View)))
        }
    }

    #[test]
    fn opaque_component_output_keeps_owner_state_for_commit() {
        let mut coordinator = FrameCoordinator::<()>::new();
        let mut build = coordinator.begin_build();
        let root = render_component::<OpaqueStateful, _>(
            &mut build,
            (),
            Children::new(|_| Ok(Body::new(Vec::new(), Vec::new()))),
            ViewSite::new("opaque-stateful", 1, 1),
        )
        .expect("opaque component view");
        let resolved = coordinator
            .prepare(root)
            .expect("prepared opaque component")
            .resolve(|_| {
                Ok::<_, ()>(UiFrame {
                    viewport: Viewport {
                        width: 1.0,
                        height: 1.0,
                    },
                    commands: Vec::new(),
                    hit_regions: Vec::new(),
                    scroll_bounds: Vec::new(),
                })
            })
            .expect("resolved opaque component");
        coordinator.commit(resolved);
        assert!(matches!(
            coordinator.take_component_lifecycle_events().as_slice(),
            [crate::ComponentLifecycleEvent::Mounted { .. }]
        ));
    }

    #[test]
    fn setup_runs_once_for_a_committed_identity() {
        SETUPS.store(0, Ordering::SeqCst);
        let mut coordinator = FrameCoordinator::new();
        publish(&mut coordinator);
        let mounted = coordinator.take_component_lifecycle_events();
        assert!(matches!(
            mounted.as_slice(),
            [crate::ComponentLifecycleEvent::Mounted { .. }]
        ));
        let scope = mounted[0].effect_scope().expect("mounted Effect scope");
        assert!(coordinator.accepts_component_effect(&scope));
        publish(&mut coordinator);
        assert!(coordinator.take_component_lifecycle_events().is_empty());
        assert!(coordinator.accepts_component_effect(&scope));
        publish_plain(&mut coordinator);
        assert!(matches!(
            coordinator.take_component_lifecycle_events().as_slice(),
            [crate::ComponentLifecycleEvent::Unmounted { .. }]
        ));
        assert!(!coordinator.accepts_component_effect(&scope));
        assert_eq!(SETUPS.load(Ordering::SeqCst), 1);
    }

    #[derive(Clone)]
    struct Item {
        id: &'static str,
        seed: u32,
    }

    #[derive(Default)]
    struct KeyedProbeProps {
        seed: Option<u32>,
    }

    struct KeyedProbe;

    impl crate::DslComponent for KeyedProbe {
        type Props = KeyedProbeProps;
        type State = u32;
        type Event = ();
        type Output = ();

        fn setup(_context: &ComponentSetupContext, props: &Self::Props) -> Self::State {
            props.seed.unwrap_or_default()
        }

        fn render<'a, A>(
            context: &mut ComponentRenderContext<'_, A>,
            _props: Self::Props,
            state: &Self::State,
            _children: Children<'a, A>,
        ) -> ViewResult<ViewOutput<A>> {
            let text =
                UiNode::new(NodeKind::Text).with_content(ContentConcern::Text(TextContent {
                    text: state.to_string(),
                    font: TextStyleRef::body(),
                    font_size: 12.0,
                    line_height: 16.0,
                    color: tela_contract::Color::BLACK,
                }));
            let root = UiNode::new(NodeKind::Frame).with_children([text]);
            let site = context.site();
            context
                .build()
                .finish(Body::new(vec![ViewChild::node(root)], Vec::new()), site)
        }
    }

    #[derive(Default)]
    struct ParentProps {
        key: Option<String>,
        seed: Option<u32>,
    }

    struct Parent;

    impl crate::DslComponent for Parent {
        type Props = ParentProps;
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
            _children: Children<'a, A>,
        ) -> ViewResult<ViewOutput<A>> {
            let build = context.build();
            ui!(build {
                <KeyedProbe seed={props.seed.unwrap_or_default()} />
            })
        }
    }

    fn render_items(build: &mut ViewBuild<()>, items: &[Item]) -> ViewResult<ViewOutput<()>> {
        ui!(build {
            <Column>
                <For each={items} key={item.id}>
                    {|item|
                        <KeyedProbe seed={item.seed} />
                    }
                </For>
            </Column>
        })
    }

    fn publish_items(coordinator: &mut FrameCoordinator<()>, items: &[Item]) {
        let mut build = coordinator.begin_build();
        let root = render_items(&mut build, items).expect("keyed component list");
        let prepared = coordinator.prepare(root).expect("prepared list");
        let resolved = prepared
            .resolve(|_| {
                Ok::<_, ()>(UiFrame {
                    viewport: Viewport {
                        width: 100.0,
                        height: 100.0,
                    },
                    commands: Vec::new(),
                    hit_regions: Vec::new(),
                    scroll_bounds: Vec::new(),
                })
            })
            .expect("resolved list");
        coordinator.commit(resolved);
    }

    fn rendered_probe_values(coordinator: &FrameCoordinator<()>) -> Vec<String> {
        coordinator
            .active()
            .expect("active list")
            .tree()
            .root()
            .children
            .iter()
            .map(|probe| match probe.children[0].content.as_ref() {
                Some(ContentConcern::Text(text)) => text.text.clone(),
                other => panic!("expected probe text, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn for_business_keys_scope_component_state_before_item_render() {
        let mut coordinator = FrameCoordinator::new();
        publish_items(
            &mut coordinator,
            &[Item { id: "a", seed: 1 }, Item { id: "b", seed: 2 }],
        );
        assert_eq!(rendered_probe_values(&coordinator), ["1", "2"]);

        publish_items(
            &mut coordinator,
            &[Item { id: "b", seed: 20 }, Item { id: "a", seed: 10 }],
        );
        assert_eq!(rendered_probe_values(&coordinator), ["2", "1"]);

        publish_items(&mut coordinator, &[Item { id: "a", seed: 10 }]);
        publish_items(&mut coordinator, &[Item { id: "b", seed: 30 }]);
        assert_eq!(rendered_probe_values(&coordinator), ["30"]);
    }

    fn render_parents(build: &mut ViewBuild<()>) -> ViewResult<ViewOutput<()>> {
        ui!(build {
            <Column>
                <Parent key={"first"} seed={1_u32} />
                <Parent key={"second"} seed={2_u32} />
            </Column>
        })
    }

    #[test]
    fn child_state_is_scoped_by_its_parent_component_instance() {
        let mut coordinator = FrameCoordinator::new();
        let mut build = coordinator.begin_build();
        let root = render_parents(&mut build).expect("parent instances");
        let prepared = coordinator.prepare(root).expect("prepared parents");
        let resolved = prepared
            .resolve(|_| {
                Ok::<_, ()>(UiFrame {
                    viewport: Viewport {
                        width: 100.0,
                        height: 100.0,
                    },
                    commands: Vec::new(),
                    hit_regions: Vec::new(),
                    scroll_bounds: Vec::new(),
                })
            })
            .expect("resolved parents");
        coordinator.commit(resolved);
        assert_eq!(rendered_probe_values(&coordinator), ["1", "2"]);
    }
}
