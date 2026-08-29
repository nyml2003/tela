use std::collections::BTreeSet;

use tela_contract::{
    KernelInteraction, NodeKind, SemanticKey, TextInputEvent, TextInputKind, TextInputSpec,
    TextSelection, UiBuildError, UiFrame, Viewport,
};
use tela_core::UiTree;
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, DslComponent, FrameCoordinator, FramePrepareError, FramedInteraction, ItemKey, Signal,
    ViewBuild, ViewBuildError, ViewOutput, ViewResult, ui, with_context,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Save,
    Open(u32),
    Search(String),
    Rename { entry_id: u32, value: String },
    ClearSearch,
}

struct State {
    count: Signal<u32>,
    child_count: Signal<u32>,
}

struct Item {
    id: u32,
    name: &'static str,
}

#[derive(Clone)]
struct WatchedItem {
    id: u32,
    value: Signal<u32>,
}

struct WatchedGroup {
    id: u32,
    items: Vec<WatchedItem>,
}

struct DomainItemId(&'static str);

impl ItemKey for DomainItemId {
    fn encode_item_key(&self) -> String {
        format!("domain:{}", self.0)
    }
}

struct DomainItem {
    id: DomainItemId,
    name: &'static str,
}

/// 测试组件：订阅 `Signal`，渲染其值（替代旧 `@watch` 指令）。
#[derive(DslComponent)]
struct WatchedCount {
    #[watch]
    count: Signal<u32>,
}

impl WatchedCount {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        ui!(build { <Text value={self.count.get().to_string()} /> })
    }
}

/// 从当前词法作用域读取一个 `String` 并渲染为文本。
///
/// derive 契约（001 §2）下作用域注入不再作为 derive 字段通道；此普通函数
/// 直接消费 `ViewContext::inject`，验证 provide/inject 机制本身仍然可用。
fn inject_label(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
    let label: String = build
        .current_scope()
        .inject::<String>(tela_ui_dsl::ViewSite::new(file!(), line!(), column!()))?
        .clone();
    ui!(build { <Text value={label} /> })
}

fn render_basics(build: &mut ViewBuild<Action>, state: &State) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column key={"browse.root"} gap={8.0}>
            <WatchedCount count={state.count.clone()} />
            <Frame clickable={true}>
                <Text value={"Save"} />
            </Frame>
        </Column>
    })
}

fn render_child(build: &mut ViewBuild<Action>, state: &State) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Frame>
            <WatchedCount count={state.child_count.clone()} />
        </Frame>
    })
}

fn temporary_count_signal(state: &State) -> Signal<u32> {
    state.count.clone()
}

fn render_temporary_watch_source(
    build: &mut ViewBuild<Action>,
    state: &State,
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Frame>
            <WatchedCount count={temporary_count_signal(state)} />
        </Frame>
    })
}

fn render_node_scoped_watch(
    build: &mut ViewBuild<Action>,
    state: &State,
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <WatchedCount count={state.count.clone()} />
        </Column>
    })
}

fn render_explicit_child_scope(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
    let site = tela_ui_dsl::ViewSite::new(file!(), line!(), column!());
    build.with_scope(
        vec![tela_ui_dsl::ProvidedValue::new::<String>("outer".to_owned())],
        site,
        |build| {
            build.with_scope(
                vec![tela_ui_dsl::ProvidedValue::new::<String>("inner".to_owned())],
                tela_ui_dsl::ViewSite::new(file!(), line!(), column!()),
                |build| ui!(build { <Frame>{ inject_label(build)? }</Frame> }),
            )
        },
    )
}

fn render_nested(build: &mut ViewBuild<Action>, state: &State) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            { render_child(build, state) }
        </Column>
    })
}

fn render_prebuilt_nested(
    build: &mut ViewBuild<Action>,
    state: &State,
) -> ViewResult<ViewOutput<Action>> {
    let child = render_child(build, state)?;
    ui!(build {
        <Column>
            { child }
        </Column>
    })
}

fn render_conditional_child(
    build: &mut ViewBuild<Action>,
    state: &State,
    show_child: bool,
) -> ViewResult<ViewOutput<Action>> {
    let child = if show_child {
        render_child(build, state)
    } else {
        Ok(ViewOutput::opaque(ViewBuild::<Action>::text_node("hidden")))
    };
    ui!(build {
        <Column>
            { child }
        </Column>
    })
}

fn render_action_target(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <ActionTarget action={Action::Save}>
            <Frame clickable={true}>
                <Text value={"Save"} />
            </Frame>
        </ActionTarget>
    })
}

fn render_watched_action_target(
    build: &mut ViewBuild<Action>,
    state: &State,
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <ActionTarget action={Action::Save}>
            <Frame clickable={true}>
                <WatchedCount count={state.count.clone()} />
            </Frame>
        </ActionTarget>
    })
}

fn render_watched_fragment(
    build: &mut ViewBuild<Action>,
    state: &State,
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Fragment>
            <Frame>
                <WatchedCount count={state.count.clone()} />
            </Frame>
        </Fragment>
    })
}

fn render_empty_watched_fragment(
    build: &mut ViewBuild<Action>,
    state: &State,
) -> ViewResult<ViewOutput<Action>> {
    let _ = state;
    ui!(build {
        <Fragment></Fragment>
    })
}

fn render_multi_root_fragment(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Fragment>
            <Frame><Text value={"Save"} /></Frame>
            <Frame><Text value={"Save"} /></Frame>
        </Fragment>
    })
}

fn render_empty_action_target(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <ActionTarget action={Action::Save}></ActionTarget>
    })
}

fn render_multi_root_action_target(
    build: &mut ViewBuild<Action>,
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <ActionTarget action={Action::Save}>
            <Frame clickable={true}><Text value={"Save"} /></Frame>
            <Frame clickable={true}><Text value={"Save"} /></Frame>
        </ActionTarget>
    })
}

fn render_duplicate_action_target(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <ActionTarget action={Action::Save}>
            <ActionTarget action={Action::Open(7)}>
                <Frame clickable={true}>
                    <Text value={"Save"} />
                </Frame>
            </ActionTarget>
        </ActionTarget>
    })
}

fn rename_entry(entry_id: u32, value: String) -> Action {
    Action::Rename { entry_id, value }
}

fn render_text_action_target(
    build: &mut ViewBuild<Action>,
    entry_id: u32,
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <ActionTarget
            on_input={with_context(entry_id, rename_entry)}
            on_submit={Action::Search}
            on_cancel={Action::ClearSearch}
        >
            <Frame input={TextInputSpec::new(TextInputKind::Text)}>
                <Text value={"Save"} />
            </Frame>
        </ActionTarget>
    })
}

fn render_prebuilt_action_target(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
    let action = render_action_target(build)?;
    ui!(build {
        <Column>
            { action }
        </Column>
    })
}

fn render_for(build: &mut ViewBuild<Action>, items: &[Item]) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={items} key={item.id}>
                {|item|
                    <Frame>
                        <Text value={item.name} />
                    </Frame>
                }
            </For>
        </Column>
    })
}

fn render_watched_for(
    build: &mut ViewBuild<Action>,
    items: &[WatchedItem],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={items} key={item.id}>
                {|item|
                    <Frame>
                        <WatchedCount count={item.value.clone()} />
                    </Frame>
                }
            </For>
        </Column>
    })
}

fn render_nested_watched_for(
    build: &mut ViewBuild<Action>,
    groups: &[WatchedGroup],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={groups} key={group.id}>
                {|group|
                    <Column>
                        <For each={group.items.iter()} key={item.id}>
                            {|item|
                                <Frame>
                                    <WatchedCount count={item.value.clone()} />
                                </Frame>
                            }
                        </For>
                    </Column>
                }
            </For>
        </Column>
    })
}

fn render_watched_virtual_list(
    build: &mut ViewBuild<Action>,
    items: &[WatchedItem],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <VirtualList
            items={items}
            total_items={20_u32}
            first_item_index={0_u32}
            item_height={32.0_f32}
            item_spacing={0.0_f32}
            overscan={0_u32}
            key={item.id}
        >
            {|item|
                <Frame>
                    <WatchedCount count={item.value.clone()} />
                </Frame>
            }
        </VirtualList>
    })
}

fn render_action_for(
    build: &mut ViewBuild<Action>,
    items: &[Item],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={items} key={item.id}>
                {|item|
                    <ActionTarget action={Action::Open(item.id)}>
                        <Frame clickable={true}>
                            <Text value={item.name} />
                        </Frame>
                    </ActionTarget>
                }
            </For>
        </Column>
    })
}

fn render_virtual_list(
    build: &mut ViewBuild<Action>,
    items: &[Item],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <VirtualList
            items={items}
            total_items={20_u32}
            first_item_index={12_u32}
            item_height={32.0_f32}
            item_spacing={4.0_f32}
            overscan={2_u32}
            key={item.id}
        >
            {|item|
                <Frame>
                    <Text value={item.name} />
                </Frame>
            }
        </VirtualList>
    })
}

fn render_out_of_range_virtual_list(
    build: &mut ViewBuild<Action>,
    items: &[Item],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <VirtualList
            items={items}
            total_items={2_u32}
            first_item_index={2_u32}
            item_height={32.0_f32}
            item_spacing={0.0_f32}
            overscan={0_u32}
            key={item.id}
        >
            {|item|
                <Frame>
                    <Text value={item.name} />
                </Frame>
            }
        </VirtualList>
    })
}

fn render_sibling_lists(
    build: &mut ViewBuild<Action>,
    items: &[Item],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={items} key={item.id}>
                {|item|
                    <Frame>
                        <Text value={item.name} />
                    </Frame>
                }
            </For>
            <For each={items} key={item.id}>
                {|item|
                    <Frame>
                        <Text value={item.name} />
                    </Frame>
                }
            </For>
        </Column>
    })
}

fn render_fragment_sibling_lists(
    build: &mut ViewBuild<Action>,
    items: &[Item],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <Fragment>
                <For each={items} key={item.id}>
                    {|item|
                        <Frame>
                            <Text value={item.name} />
                        </Frame>
                    }
                </For>
                <For each={items} key={item.id}>
                    {|item|
                        <Frame>
                            <Text value={item.name} />
                        </Frame>
                    }
                </For>
            </Fragment>
        </Column>
    })
}

fn render_domain_key_for(
    build: &mut ViewBuild<Action>,
    items: &[DomainItem],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={items} key={item.id}>
                {|item|
                    <Frame>
                        <Text value={item.name} />
                    </Frame>
                }
            </For>
        </Column>
    })
}

fn render_conflicting_for(
    build: &mut ViewBuild<Action>,
    items: &[Item],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={items} key={item.id}>
                {|item|
                    <Frame key={"already-key"}>
                        <Text value={item.name} />
                    </Frame>
                }
            </For>
        </Column>
    })
}

fn render_primitive_for(
    build: &mut ViewBuild<Action>,
    items: &[Item],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={items} key={item.id}>
                {|item| <Text value={item.name} />}
            </For>
        </Column>
    })
}

fn render_empty_fragment_for(
    build: &mut ViewBuild<Action>,
    items: &[Item],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={items} key={item.id}>
                {|item| <Fragment></Fragment>}
            </For>
        </Column>
    })
}

fn render_multi_root_fragment_for(
    build: &mut ViewBuild<Action>,
    items: &[Item],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={items} key={item.id}>
                {|item|
                    <Fragment>
                        <Frame><Text value={"Save"} /></Frame>
                        <Frame><Text value={"Save"} /></Frame>
                    </Fragment>
                }
            </For>
        </Column>
    })
}

fn empty_resolved_frame() -> UiFrame {
    UiFrame {
        viewport: Viewport {
            width: 1.0,
            height: 1.0,
        },
        commands: Vec::new(),
        hit_regions: Vec::new(),
        scroll_bounds: Vec::new(),
    }
}

fn publish(coordinator: &mut FrameCoordinator<Action>, root: ViewOutput<Action>) {
    let prepared = coordinator.prepare(root).expect("candidate frame");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_resolved_frame()))
        .expect("resolved frame");
    coordinator.commit(resolved);
}

#[test]
fn directives_build_a_real_root_and_watch_its_resolved_key() {
    let state = State {
        count: Signal::new(3),
        child_count: Signal::new(0),
    };
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    let root = render_basics(&mut build, &state).expect("view");
    publish(&mut coordinator, root);

    assert!(coordinator.runtime().take_dirty().is_empty());
    state.count.set(4);
    assert_eq!(
        coordinator.runtime().take_dirty(),
        BTreeSet::from([SemanticKey("/0/".to_owned())])
    );
}

#[test]
fn temporary_watch_source_is_cloned_before_its_reference_ends() {
    let state = State {
        count: Signal::new(3),
        child_count: Signal::new(0),
    };
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    let root = render_temporary_watch_source(&mut build, &state).expect("view");
    publish(&mut coordinator, root);

    state.count.set(4);
    assert_eq!(
        coordinator.runtime().take_dirty(),
        BTreeSet::from([SemanticKey("/0/".to_owned())])
    );
}

#[test]
fn nested_ui_plan_is_rebased_to_the_real_opaque_child_root() {
    let state = State {
        count: Signal::new(0),
        child_count: Signal::new(1),
    };
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    let root = render_nested(&mut build, &state).expect("view");
    publish(&mut coordinator, root);

    state.child_count.set(2);
    assert_eq!(
        coordinator.runtime().take_dirty(),
        BTreeSet::from([SemanticKey("/0/0/".to_owned())])
    );
}

#[test]
fn prebuilt_child_view_keeps_its_watch_plan_when_inserted_later() {
    let state = State {
        count: Signal::new(0),
        child_count: Signal::new(1),
    };
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    let root = render_prebuilt_nested(&mut build, &state).expect("view");
    publish(&mut coordinator, root);

    state.child_count.set(2);
    assert_eq!(
        coordinator.runtime().take_dirty(),
        BTreeSet::from([SemanticKey("/0/0/".to_owned())])
    );
}

#[test]
fn conditional_child_rebases_and_releases_its_plan_with_the_real_branch() {
    let state = State {
        count: Signal::new(0),
        child_count: Signal::new(1),
    };
    let mut coordinator = FrameCoordinator::new();

    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_conditional_child(&mut build, &state, true).expect("visible child"),
    );
    state.child_count.set(2);
    assert_eq!(
        coordinator.runtime().take_dirty(),
        BTreeSet::from([SemanticKey("/0/0/".to_owned())])
    );

    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_conditional_child(&mut build, &state, false).expect("hidden child"),
    );
    state.child_count.set(3);
    assert!(coordinator.runtime().take_dirty().is_empty());

    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_conditional_child(&mut build, &state, true).expect("visible child again"),
    );
    state.child_count.set(4);
    assert_eq!(
        coordinator.runtime().take_dirty(),
        BTreeSet::from([SemanticKey("/0/0/".to_owned())])
    );
}

#[test]
fn watch_on_a_real_node_body_is_anchored_to_that_node() {
    let state = State {
        count: Signal::new(3),
        child_count: Signal::new(0),
    };
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    let root = render_node_scoped_watch(&mut build, &state).expect("view");
    publish(&mut coordinator, root);

    state.count.set(4);
    assert_eq!(
        coordinator.runtime().take_dirty(),
        BTreeSet::from([SemanticKey("/0/".to_owned())])
    );
}

#[test]
fn nested_explicit_ui_scope_inherits_its_parent_context() {
    let mut build = ViewBuild::<Action>::new();

    let view = render_explicit_child_scope(&mut build).expect("view");
    let tree = UiTree::new(view.node().clone())
        .expect("nested explicit scope must resolve the parent provider");

    // Frame > Text 两层；嵌套 with_scope（outer→inner）不添加树层，
    // inject_label 解析最近作用域的 "inner"（缺失则构建失败）。
    assert_eq!(
        tree.keys(),
        [SemanticKey("/".to_owned()), SemanticKey("/0/".to_owned()),]
    );
}

#[test]
fn action_target_routes_clicks_without_storing_a_callback_in_the_tree() {
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    let root = render_action_target(&mut build).expect("view");
    publish(&mut coordinator, root);
    let node_id = coordinator
        .active()
        .expect("active frame")
        .tree()
        .node_id_for_key(&SemanticKey("/".to_owned()))
        .expect("root id");

    assert_eq!(
        coordinator.dispatch_interaction(&FramedInteraction::new(
            coordinator.active().expect("active frame").token(),
            KernelInteraction::Activate { node_id },
        )),
        Some(Action::Save)
    );
}

#[test]
fn top_level_watch_and_action_target_anchor_to_the_unchanged_real_root() {
    let state = State {
        count: Signal::new(3),
        child_count: Signal::new(0),
    };
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_watched_action_target(&mut build, &state).expect("watched action target"),
    );

    let active = coordinator.active().expect("active frame");
    assert_eq!(
        active.tree().keys(),
        [SemanticKey("/".to_owned()), SemanticKey("/0/".to_owned()),]
    );
    let token = active.token();
    let node_id = active
        .tree()
        .node_id_for_key(&SemanticKey("/".to_owned()))
        .expect("the ActionTarget child is the real root");

    state.count.set(4);
    assert_eq!(
        coordinator.runtime().take_dirty(),
        BTreeSet::from([SemanticKey("/0/".to_owned())])
    );
    assert_eq!(
        coordinator.dispatch_interaction(&FramedInteraction::new(
            token,
            KernelInteraction::Activate { node_id }
        )),
        Some(Action::Save)
    );
}

#[test]
fn top_level_fragment_does_not_add_an_auto_path_layer() {
    let state = State {
        count: Signal::new(3),
        child_count: Signal::new(0),
    };
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_watched_fragment(&mut build, &state).expect("watched fragment"),
    );

    assert_eq!(
        coordinator.active().expect("active frame").tree().keys(),
        [SemanticKey("/".to_owned()), SemanticKey("/0/".to_owned()),]
    );
    state.count.set(4);
    assert_eq!(
        coordinator.runtime().take_dirty(),
        BTreeSet::from([SemanticKey("/0/".to_owned())])
    );
}

#[test]
fn top_level_fragment_requires_exactly_one_real_root() {
    let state = State {
        count: Signal::new(0),
        child_count: Signal::new(0),
    };
    let mut build = ViewBuild::new();
    assert!(matches!(
        render_empty_watched_fragment(&mut build, &state),
        Err(ViewBuildError::ExpectedSingleRoot { actual: 0, .. })
    ));

    let mut build = ViewBuild::new();
    assert!(matches!(
        render_multi_root_fragment(&mut build),
        Err(ViewBuildError::ExpectedSingleRoot { actual: 2, .. })
    ));
}

#[test]
fn action_target_requires_exactly_one_real_child() {
    let mut build = ViewBuild::new();
    assert!(matches!(
        render_empty_action_target(&mut build),
        Err(ViewBuildError::ActionTargetRequiresSingleRoot { actual: 0, .. })
    ));

    let mut build = ViewBuild::new();
    assert!(matches!(
        render_multi_root_action_target(&mut build),
        Err(ViewBuildError::ActionTargetRequiresSingleRoot { actual: 2, .. })
    ));
}

#[test]
fn duplicate_action_targets_for_one_real_root_are_rejected() {
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    let root = render_duplicate_action_target(&mut build).expect("lowered duplicate target");

    assert!(matches!(
        coordinator.prepare(root),
        Err(FramePrepareError::Plans(
            ViewBuildError::DuplicateActionBinding { .. }
        ))
    ));
}

#[test]
fn text_action_target_routes_declared_payloads_and_pure_context() {
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    let root = render_text_action_target(&mut build, 42).expect("text target view");
    publish(&mut coordinator, root);

    let active = coordinator.active().expect("active frame");
    let token = active.token();
    let node_id = active
        .tree()
        .node_id_for_key(&SemanticKey("/".to_owned()))
        .expect("text input root id");

    assert_eq!(
        coordinator.dispatch_interaction(&FramedInteraction::new(
            token,
            KernelInteraction::TextInput {
                node_id,
                event: TextInputEvent::Edit {
                    value: "draft".to_owned(),
                    selection: TextSelection::collapsed(5),
                    composing: true,
                },
            },
        )),
        Some(Action::Rename {
            entry_id: 42,
            value: "draft".to_owned(),
        })
    );
    assert_eq!(
        coordinator.dispatch_interaction(&FramedInteraction::new(
            token,
            KernelInteraction::TextInput {
                node_id,
                event: TextInputEvent::Commit {
                    value: "confirmed".to_owned(),
                    selection: TextSelection::collapsed(9),
                },
            },
        )),
        Some(Action::Search("confirmed".to_owned()))
    );
    assert_eq!(
        coordinator.dispatch_interaction(&FramedInteraction::new(
            token,
            KernelInteraction::TextInput {
                node_id,
                event: TextInputEvent::Cancel {
                    selection: TextSelection::collapsed(0),
                },
            },
        )),
        Some(Action::ClearSearch)
    );
}

#[test]
fn prebuilt_child_view_keeps_its_action_plan_when_inserted_later() {
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    let root = render_prebuilt_action_target(&mut build).expect("view");
    publish(&mut coordinator, root);
    let node_id = coordinator
        .active()
        .expect("active frame")
        .tree()
        .node_id_for_key(&SemanticKey("/0/".to_owned()))
        .expect("prebuilt target root id");

    assert_eq!(
        coordinator.dispatch_interaction(&FramedInteraction::new(
            coordinator.active().expect("active frame").token(),
            KernelInteraction::Activate { node_id },
        )),
        Some(Action::Save)
    );
}

#[test]
fn for_watches_follow_business_keys_through_reorder_and_release_removed_items() {
    let first_signal = Signal::new(7_u32);
    let second_signal = Signal::new(8_u32);
    let initial = [
        WatchedItem {
            id: 7,
            value: first_signal.clone(),
        },
        WatchedItem {
            id: 8,
            value: second_signal.clone(),
        },
    ];
    let mut coordinator = FrameCoordinator::new();

    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_watched_for(&mut build, &initial).expect("initial For view"),
    );
    first_signal.set(70);
    assert!(!coordinator.runtime().take_dirty().is_empty());

    let reordered = [
        WatchedItem {
            id: 8,
            value: second_signal.clone(),
        },
        WatchedItem {
            id: 7,
            value: first_signal.clone(),
        },
    ];
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_watched_for(&mut build, &reordered).expect("reordered For view"),
    );
    first_signal.set(71);
    assert!(!coordinator.runtime().take_dirty().is_empty());

    let remaining = [WatchedItem {
        id: 8,
        value: second_signal.clone(),
    }];
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_watched_for(&mut build, &remaining).expect("filtered For view"),
    );
    first_signal.set(72);
    assert!(coordinator.runtime().take_dirty().is_empty());
    second_signal.set(80);
    assert!(!coordinator.runtime().take_dirty().is_empty());
}

#[test]
fn nested_for_keys_preserve_the_outer_and_inner_business_identity() {
    let first_inner_signal = Signal::new(11_u32);
    let second_inner_signal = Signal::new(12_u32);
    let sibling_inner_signal = Signal::new(21_u32);
    let mut coordinator = FrameCoordinator::new();

    let initial = vec![
        WatchedGroup {
            id: 7,
            items: vec![
                WatchedItem {
                    id: 11,
                    value: first_inner_signal.clone(),
                },
                WatchedItem {
                    id: 12,
                    value: second_inner_signal.clone(),
                },
            ],
        },
        WatchedGroup {
            id: 8,
            items: vec![WatchedItem {
                id: 11,
                value: sibling_inner_signal.clone(),
            }],
        },
    ];
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_nested_watched_for(&mut build, &initial).expect("initial nested For view"),
    );
    assert!(
        coordinator
            .active()
            .expect("active nested For frame")
            .tree()
            .keys()
            .contains(&SemanticKey("/0/0/0/".to_owned()))
    );
    assert!(
        coordinator
            .active()
            .expect("active nested For frame")
            .tree()
            .keys()
            .contains(&SemanticKey("/0/1/0/".to_owned()))
    );

    first_inner_signal.set(110);
    assert!(!coordinator.runtime().take_dirty().is_empty());

    let reordered = vec![
        WatchedGroup {
            id: 8,
            items: vec![WatchedItem {
                id: 11,
                value: sibling_inner_signal.clone(),
            }],
        },
        WatchedGroup {
            id: 7,
            items: vec![
                WatchedItem {
                    id: 12,
                    value: second_inner_signal.clone(),
                },
                WatchedItem {
                    id: 11,
                    value: first_inner_signal.clone(),
                },
            ],
        },
    ];
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_nested_watched_for(&mut build, &reordered).expect("reordered nested For view"),
    );
    first_inner_signal.set(111);
    assert!(!coordinator.runtime().take_dirty().is_empty());

    let only_sibling = vec![WatchedGroup {
        id: 8,
        items: vec![WatchedItem {
            id: 11,
            value: sibling_inner_signal,
        }],
    }];
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_nested_watched_for(&mut build, &only_sibling).expect("filtered nested For view"),
    );
    first_inner_signal.set(112);
    assert!(coordinator.runtime().take_dirty().is_empty());
}

#[test]
fn virtual_list_window_release_removes_unloaded_item_watches() {
    let first_signal = Signal::new(7_u32);
    let second_signal = Signal::new(8_u32);
    let visible = [
        WatchedItem {
            id: 7,
            value: first_signal.clone(),
        },
        WatchedItem {
            id: 8,
            value: second_signal.clone(),
        },
    ];
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_watched_virtual_list(&mut build, &visible).expect("visible window"),
    );

    let shifted_window = [WatchedItem {
        id: 8,
        value: second_signal.clone(),
    }];
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_watched_virtual_list(&mut build, &shifted_window).expect("shifted window"),
    );

    first_signal.set(70);
    assert!(coordinator.runtime().take_dirty().is_empty());
    second_signal.set(80);
    assert!(!coordinator.runtime().take_dirty().is_empty());
}

#[test]
fn for_action_targets_rebind_to_the_same_business_item_after_reorder() {
    let initial = [Item { id: 7, name: "A" }, Item { id: 8, name: "B" }];
    let mut coordinator = FrameCoordinator::new();
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_action_for(&mut build, &initial).expect("initial action list"),
    );
    let first_token = coordinator.active().expect("active frame").token();
    let first_node = coordinator
        .active()
        .expect("active frame")
        .tree()
        .node_id_for_key(&SemanticKey("/@for-0/7".to_owned()))
        .expect("first item root");
    assert_eq!(
        coordinator.dispatch_interaction(&FramedInteraction::new(
            first_token,
            KernelInteraction::Activate {
                node_id: first_node
            },
        )),
        Some(Action::Open(7))
    );

    let reordered = [Item { id: 8, name: "B" }, Item { id: 7, name: "A" }];
    let mut build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render_action_for(&mut build, &reordered).expect("reordered action list"),
    );
    let token = coordinator.active().expect("active frame").token();
    let node = coordinator
        .active()
        .expect("active frame")
        .tree()
        .node_id_for_key(&SemanticKey("/@for-0/7".to_owned()))
        .expect("reordered item root");
    assert_eq!(
        coordinator.dispatch_interaction(&FramedInteraction::new(
            token,
            KernelInteraction::Activate { node_id: node }
        )),
        Some(Action::Open(7))
    );
}

#[test]
fn for_keys_are_scoped_by_the_resolved_parent_key() {
    let items = [Item { id: 7, name: "A" }, Item { id: 8, name: "B" }];
    let mut build = ViewBuild::new();
    let view = render_for(&mut build, &items).expect("view");
    let tree = UiTree::new(view.node().clone()).expect("valid tree");

    assert_eq!(
        tree.keys(),
        [
            SemanticKey("/".to_owned()),
            SemanticKey("/@for-0/7".to_owned()),
            SemanticKey("/0/0/".to_owned()),
            SemanticKey("/@for-0/8".to_owned()),
            SemanticKey("/1/0/".to_owned()),
        ]
    );
}

#[test]
fn virtual_list_requires_an_explicit_window_contract_and_scopes_item_keys() {
    let items = [Item { id: 7, name: "A" }, Item { id: 8, name: "B" }];
    let mut build = ViewBuild::new();
    let view = render_virtual_list(&mut build, &items).expect("view");
    let tree = UiTree::new(view.node().clone()).expect("valid virtual list window");

    let NodeKind::VirtualListView(spec) = &tree.root().kind else {
        panic!("the DSL root must be a VirtualListView");
    };
    assert_eq!(spec.total_items, 20);
    assert_eq!(spec.first_item_index, 12);
    assert_eq!(spec.item_height, 32.0);
    assert_eq!(spec.item_spacing, 4.0);
    assert_eq!(spec.overscan, 2);
    assert!(tree.keys().contains(&SemanticKey("/@for-0/7".to_owned())));
    assert!(tree.keys().contains(&SemanticKey("/@for-0/8".to_owned())));
}

#[test]
fn virtual_list_window_must_fit_the_explicit_total_range() {
    let items = [Item { id: 7, name: "A" }];
    let mut build = ViewBuild::new();
    let root = render_out_of_range_virtual_list(&mut build, &items).expect("lowering");

    assert!(matches!(
        UiTree::new(root.node().clone()),
        Err(UiBuildError::InvalidVirtualListRange)
    ));
}

#[test]
fn sibling_for_blocks_can_reuse_the_same_local_business_key() {
    let items = [Item { id: 7, name: "A" }];
    let mut build = ViewBuild::new();
    let view = render_sibling_lists(&mut build, &items).expect("view");
    let tree = UiTree::new(view.node().clone()).expect("sibling collection keys must not collide");

    assert!(tree.keys().contains(&SemanticKey("/@for-0/7".to_owned())));
    assert!(tree.keys().contains(&SemanticKey("/@for-1/7".to_owned())));
}

#[test]
fn transparent_fragment_shares_the_parent_collection_namespace() {
    let items = [Item { id: 7, name: "A" }];
    let mut build = ViewBuild::new();
    let view = render_fragment_sibling_lists(&mut build, &items).expect("view");
    let tree = UiTree::new(view.node().clone()).expect("fragment collection keys must not collide");

    assert!(tree.keys().contains(&SemanticKey("/@for-0/7".to_owned())));
    assert!(tree.keys().contains(&SemanticKey("/@for-1/7".to_owned())));
}

#[test]
fn domain_item_key_is_converted_without_exposing_key_segment() {
    let items = [DomainItem {
        id: DomainItemId("first"),
        name: "A",
    }];
    let mut build = ViewBuild::new();
    let view = render_domain_key_for(&mut build, &items).expect("view");
    let tree = UiTree::new(view.node().clone()).expect("domain key must be accepted");

    assert!(
        tree.keys()
            .contains(&SemanticKey("/@for-0/domain:first".to_owned()))
    );
}

#[test]
fn for_rejects_a_root_that_already_owns_identity() {
    let items = [Item { id: 7, name: "A" }];
    let mut build = ViewBuild::new();

    assert!(matches!(
        render_conflicting_for(&mut build, &items),
        Err(ViewBuildError::ForItemIdentityConflict { .. })
    ));
}

#[test]
fn for_rejects_a_primitive_item_root() {
    let items = [Item { id: 7, name: "A" }];
    let mut build = ViewBuild::new();

    assert!(matches!(
        render_primitive_for(&mut build, &items),
        Err(ViewBuildError::ForItemRootCannotCarryIdentity { .. })
    ));
}

#[test]
fn for_requires_one_real_item_root_after_fragment_lowering() {
    let items = [Item { id: 7, name: "A" }];
    let mut build = ViewBuild::new();
    assert!(matches!(
        render_empty_fragment_for(&mut build, &items),
        Err(ViewBuildError::ForItemRequiresSingleRoot { actual: 0, .. })
    ));

    let mut build = ViewBuild::new();
    assert!(matches!(
        render_multi_root_fragment_for(&mut build, &items),
        Err(ViewBuildError::ForItemRequiresSingleRoot { actual: 2, .. })
    ));
}

fn render_view_container(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <View
            key={"view.container"}
            width={120.0}
            height={48.0}
            fill={tela_contract::Fill::Solid(tela_contract::Color::BLUE)}
        >
            <Text value={"boxed"} />
        </View>
    })
}

fn render_empty_view(build: &mut ViewBuild<Action>) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <View
            key={"view.empty"}
            width={40.0}
            height={2.0}
            fill={tela_contract::Fill::Solid(tela_contract::Color::BLACK)}
        />
    })
}

#[test]
fn view_renders_as_a_boxed_container() {
    let mut build = ViewBuild::new();
    let view = render_view_container(&mut build).expect("view container");
    let tree = UiTree::new(view.node().clone()).expect("view tree");
    assert_eq!(view.node().kind, tela_contract::NodeKind::View);
    assert_eq!(view.node().children.len(), 1);
    assert!(tree.keys().iter().any(|key| key.0 == "view.container"));
}

#[test]
fn empty_view_builds_as_a_decoration_block() {
    let mut build = ViewBuild::new();
    let view = render_empty_view(&mut build).expect("empty view");
    assert_eq!(view.node().kind, tela_contract::NodeKind::View);
    assert!(view.node().children.is_empty());
    let tree = UiTree::new(view.node().clone()).expect("empty view tree");
    assert!(tree.keys().iter().any(|key| key.0 == "view.empty"));
}
