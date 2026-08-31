//! 声明式组件的 setup/render/handler 生命周期协议。
//!
//! 状态由 `ViewBuild` 的候选 owner 帧保存，应用不需要持有 owner 或身份对象。

use std::rc::Rc;

use crate::{
    AnimationController, AnimationSample, Children, DslComponent, Interpolate, TransitionTarget,
    UiSpec, ViewBuild, ViewContext, ViewOutput, ViewResult, ViewSite,
    candidate::{ComponentLease, OutputConnection},
    component::NoOutput,
    inbox::ComponentEventSender,
    owner::{ComponentIdentity, ComponentState, component_event_route},
    runtime::{StructuralDirtyTarget, WatchSource},
    view::StructuralWatchHandle,
};

/// setup 阶段可读取的只读上下文。
pub struct ComponentSetupContext<E> {
    scope: Rc<ViewContext>,
    event_sender: ComponentEventSender<E>,
}

impl<E> ComponentSetupContext<E> {
    pub(crate) fn new(scope: Rc<ViewContext>, event_sender: ComponentEventSender<E>) -> Self {
        Self {
            scope,
            event_sender,
        }
    }

    /// 当前 provide/inject 作用域快照。
    pub fn scope(&self) -> &ViewContext {
        &self.scope
    }

    /// 从当前作用域读取已提供值。
    pub fn inject<T: Clone + 'static>(&self, site: ViewSite) -> Option<T> {
        self.scope.inject::<T>(site).ok().cloned()
    }

    /// 返回一个只可向当前组件实例投递 `E` 的类型化 sender。
    ///
    /// sender 可以保存到 State 并交给该组件生命周期管理的后台任务。它只会进入 UI
    /// 调度邮箱，绝不会直接调用 handler；组件卸载、替换或重建后旧 sender 的 event 会
    /// 由内部 lease 检查静默丢弃。
    pub fn event_sender(&self) -> ComponentEventSender<E> {
        self.event_sender.clone()
    }
}

/// assemble 阶段上下文；只允许通过 `ViewBuild` 生成节点和附属计划。
pub struct ComponentAssembleContext<'a, A> {
    build: &'a mut ViewBuild<A>,
    site: ViewSite,
    /// 当前候选组件实例的私有身份。
    identity: ComponentIdentity,
    /// 当前候选组件实例的完整 lease。它只用于内置透明结构 source 的失效目标，不能
    /// 转化为业务路由或交给普通组件。
    lease: ComponentLease,
}

impl<'a, A> ComponentAssembleContext<'a, A> {
    pub(crate) fn new(
        build: &'a mut ViewBuild<A>,
        site: ViewSite,
        identity: ComponentIdentity,
        lease: ComponentLease,
    ) -> Self {
        Self {
            build,
            site,
            identity,
            lease,
        }
    }

    /// 当前组件调用点。
    pub fn site(&self) -> ViewSite {
        self.site
    }

    /// 当前候选组件实例的框架内部身份。
    ///
    /// 仅 DSL 内置结构组件使用它建立透明 collection namespace；业务组件不应把它
    /// 保存、比较或暴露到自己的 API。
    pub(crate) fn identity(&self) -> &ComponentIdentity {
        &self.identity
    }

    /// Creates one lease-owned invalidation edge for a built-in transparent structure.
    ///
    /// `Show` and `For` have no physical root, so a normal node-anchored `WatchHandle` would
    /// either fail for an empty structure or accidentally borrow a child identity. This method
    /// is crate-private and intentionally accepts an already erased source: external `UiSpec`s
    /// never receive a structural target capability.
    pub(crate) fn structural_watch(&self, source: Box<dyn WatchSource>) -> StructuralWatchHandle {
        StructuralWatchHandle::new(
            source,
            StructuralDirtyTarget::new(self.lease.clone()),
            self.site,
            self.identity.scope(),
        )
    }

    /// 访问底层 ViewBuild。组件只能在 render 中用它构造候选节点。
    pub fn build(&mut self) -> &mut ViewBuild<A> {
        self.build
    }

    /// 解析组件私有、可跨帧持久的隐式 transition。
    ///
    /// `key` 只需在当前组件内稳定且唯一。控制器状态位于候选 owner 帧；候选失败不会
    /// 污染 active 状态。返回值只影响作者选择写入的视觉槽位，不会改变命中盒。
    pub fn transition<T>(
        &mut self,
        key: impl Into<String>,
        target: TransitionTarget<T>,
    ) -> AnimationSample<T>
    where
        T: Interpolate + PartialEq + Clone + 'static,
    {
        let transition_key = format!("transition:{}", key.into());
        let transition_identity = self.build.component_identity(
            std::any::type_name::<AnimationController<T>>(),
            self.site,
            Some(transition_key.as_str()),
        );
        let state = self.build.local_state_for(transition_identity, || {
            AnimationController::new(target.value().clone())
        });
        let clock = self.build.animation_clock();
        let sample = state.update(|controller| controller.resolve(clock, target));
        self.build.request_animation(sample.schedule);
        sample
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

/// 由 `ui!` 生成的 Props 初始化锚点。
///
/// 关联的 `UiSpec<A>` 依赖当前 `ViewBuild<A>` 的应用 Action 类型；把这个关系放在
/// 函数参数里，可让 Rust 从 `build` 推断 `A`，而不是要求宏在 Props 类型位置留下多个
/// 无约束的 `_`。
#[doc(hidden)]
pub fn default_component_props<C, A>(_build: &ViewBuild<A>) -> <C::UiSpec<A> as UiSpec<A>>::Props
where
    C: DslComponent,
    A: 'static,
    C::UiSpec<A>: UiSpec<A>,
{
    Default::default()
}

/// 由 `ui!` 生成的 `@output` mapper 类型推断锚点。
///
/// 它不包装函数，也不捕获任何调用点数据；只把 `ViewBuild<A>` 所携带的应用类型与
/// `C::UiSpec<A>::Output` 放进同一个普通 Rust 函数签名中。
#[doc(hidden)]
pub fn component_output_mapper<C, A, M>(
    _build: &ViewBuild<A>,
    mapper: fn(<C::UiSpec<A> as UiSpec<A>>::Output) -> M,
) -> fn(<C::UiSpec<A> as UiSpec<A>>::Output) -> M
where
    C: DslComponent,
    A: 'static,
    M: 'static,
    C::UiSpec<A>: UiSpec<A>,
{
    mapper
}

/// 按 DSL 调用点取得组件私有状态，并执行统一的 `UiSpec::assemble` 生命周期。
pub fn assemble_component<'a, C, A>(
    build: &mut ViewBuild<A>,
    props: <C::UiSpec<A> as UiSpec<A>>::Props,
    children: Children<'a, A>,
    site: ViewSite,
) -> ViewResult<ViewOutput<A>>
where
    C: DslComponent + 'static,
    A: 'static,
    C::UiSpec<A>: UiSpec<A> + 'static,
    <C::UiSpec<A> as UiSpec<A>>::Output: NoOutput,
{
    let identity = component_identity::<C, A>(build, &props, site);
    let lease = build.register_component_lease(identity.clone());
    let output = build.ignored_output::<<C::UiSpec<A> as UiSpec<A>>::Output>(lease.clone(), site);
    assemble_component_bound::<C, A, _>(build, props, children, identity, lease, output, site)
}

/// 装配组件并把其公开 Output 接到调用点提供的静态 mapper。
///
/// 此入口是 `@output={function_path}` 的 macro assemble 锚点。输入适配仍完全由
/// `UiSpec` 声明；调用点只提供 Output 的显式转换，不接触孩子的私有 Event 或 State。
#[doc(hidden)]
pub fn assemble_component_with_output<'a, C, A, M>(
    build: &mut ViewBuild<A>,
    props: <C::UiSpec<A> as UiSpec<A>>::Props,
    children: Children<'a, A>,
    output: fn(<C::UiSpec<A> as UiSpec<A>>::Output) -> M,
    mapper_name: &'static str,
    site: ViewSite,
) -> ViewResult<ViewOutput<A>>
where
    C: DslComponent + 'static,
    A: 'static,
    M: 'static,
    C::UiSpec<A>: UiSpec<A> + 'static,
{
    let identity = component_identity::<C, A>(build, &props, site);
    let lease = build.register_component_lease(identity.clone());
    let output = build.bind_output(lease.clone(), output, mapper_name, site)?;
    assemble_component_bound::<C, A, M>(build, props, children, identity, lease, output, site)
}

fn component_identity<C, A>(
    build: &ViewBuild<A>,
    props: &<C::UiSpec<A> as UiSpec<A>>::Props,
    site: ViewSite,
) -> ComponentIdentity
where
    C: DslComponent,
    A: 'static,
    C::UiSpec<A>: UiSpec<A>,
{
    build.component_identity(
        std::any::type_name::<C>(),
        site,
        <C::UiSpec<A> as UiSpec<A>>::identity_key(props).as_deref(),
    )
}

fn assemble_component_bound<'a, C, A, M>(
    build: &mut ViewBuild<A>,
    props: <C::UiSpec<A> as UiSpec<A>>::Props,
    children: Children<'a, A>,
    identity: ComponentIdentity,
    lease: crate::candidate::ComponentLease,
    output_connection: OutputConnection<<C::UiSpec<A> as UiSpec<A>>::Output, A, M>,
    site: ViewSite,
) -> ViewResult<ViewOutput<A>>
where
    C: DslComponent + 'static,
    A: 'static,
    M: 'static,
    C::UiSpec<A>: UiSpec<A> + 'static,
{
    let memo_active = build.memo_enabled();
    if memo_active {
        build.memo_component_started(identity.clone());
    }
    let setup_scope = build.current_scope();
    let event_sender =
        build.component_event_sender::<<C::UiSpec<A> as UiSpec<A>>::Event>(lease.clone(), site);
    let state: ComponentState<<C::UiSpec<A> as UiSpec<A>>::State> =
        build.local_state_for(identity.clone(), || {
            <C::UiSpec<A> as UiSpec<A>>::setup(
                &ComponentSetupContext::new(setup_scope, event_sender),
                &props,
            )
        });
    let route_props = props.clone();
    let assembled = build.with_component_identity(&identity, |build| {
        build.with_output_scope::<<C::UiSpec<A> as UiSpec<A>>::Event, _>(
            lease.clone(),
            <C::UiSpec<A> as UiSpec<A>>::OWNS_CHILD_OUTPUT_SCOPE,
            |build| {
                let mut context =
                    ComponentAssembleContext::new(build, site, identity.clone(), lease.clone());
                <C::UiSpec<A> as UiSpec<A>>::assemble(&mut context, props, &state.get(), children)
            },
        )
    });
    if memo_active {
        build.memo_component_finished();
    }
    let output = assembled?;
    // A component may intentionally return an opaque or kit-provided node. The owner frame is
    // still part of the component's assembled output; otherwise state materialized by
    // setup/assemble
    // is dropped before FrameCoordinator can commit it and later input cannot dispatch.
    let output = output
        .with_owner_frame(
            build
                .owner_frame
                .clone()
                .expect("component render must retain the candidate owner frame"),
        )
        .with_candidate_assembly(build.candidate_assembly());
    // 所有组件都安装自身 Event handler。普通 HostInput 与 child Output 只会命中部分
    // 路由，而 setup 签发的 sender 也需要让透明结构组件接收“自己的 Event”。这不是
    // 放宽跨组件路由：sender 的 lease 永远只能解析回创建它的这个实例。
    build.register_component_event_route(component_event_route::<C, A, M>(
        identity.clone(),
        lease.clone(),
        site,
        route_props.clone(),
        output_connection.clone(),
    ));
    <C::UiSpec<A> as UiSpec<A>>::wire_output(
        output,
        identity,
        &route_props,
        output_connection,
        site,
    )
}
