//! retained 求值语义（默认，无注解）的一致性与粒度集成测试。
//!
//! 对齐 `m5_update.rs` 的 Full/Dirty 一致性风格：同一 signal 变更序列下，
//! retained 帧与全量渲染帧必须产出完全相同的树，且 render 只重跑订阅被标脏的
//! 组件（derive 契约：输入只有 `#[watch]` 边，命中判定是纯 SignalId 比较）。

use std::{cell::Cell, collections::BTreeSet};

use tela_contract::{NodeKind, SemanticKey, UiFrame, UiNode, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, DslComponent, FrameCoordinator, Signal, ViewBuild, ViewChild, ViewOutput, ViewResult,
    ViewSite, ui,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    #[allow(dead_code)]
    Ping,
}

// 计数器用 thread_local：集成测试并行运行，静态全局会被相邻用例串扰。
thread_local! {
    static RENDER_A: Cell<usize> = const { Cell::new(0) };
    static RENDER_B: Cell<usize> = const { Cell::new(0) };
    static RENDER_INNER: Cell<usize> = const { Cell::new(0) };
}

fn bump_a() {
    RENDER_A.with(|count| count.set(count.get() + 1));
}

fn bump_b() {
    RENDER_B.with(|count| count.set(count.get() + 1));
}

fn bump_inner() {
    RENDER_INNER.with(|count| count.set(count.get() + 1));
}

fn renders_a() -> usize {
    RENDER_A.with(Cell::get)
}

fn renders_b() -> usize {
    RENDER_B.with(Cell::get)
}

fn renders_inner() -> usize {
    RENDER_INNER.with(Cell::get)
}

/// 面板 A：订阅独立 signal；默认 retained（无 children 即参与缓存）。
#[derive(DslComponent)]
struct PanelA {
    #[watch]
    value: Signal<u32>,
}

impl PanelA {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        bump_a();
        ui!(build {
            <Text value={format!("A:{}", self.value.get())} />
        })
    }
}

/// A non-primitive retained root, so it is valid as a keyed `<For>` item root.
#[derive(DslComponent)]
struct ForPanel {
    #[watch]
    value: Signal<u32>,
}

impl ForPanel {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        bump_a();
        ui!(build {
            <Column>
                <Text value={format!("F:{}", self.value.get())} />
            </Column>
        })
    }
}

/// 面板 B：A 的不相关兄弟组件。
#[derive(DslComponent)]
struct PanelB {
    #[watch]
    value: Signal<u32>,
}

impl PanelB {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        bump_b();
        ui!(build {
            <Text value={format!("B:{}", self.value.get())} />
        })
    }
}

/// 嵌套内层：Outer 的直接子组件，订阅独立的 b。
#[derive(DslComponent)]
struct InnerPanel {
    #[watch]
    value: Signal<u32>,
}

impl InnerPanel {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        bump_inner();
        ui!(build {
            <Text value={format!("I:{}", self.value.get())} />
        })
    }
}

/// 嵌套外层：订阅 a，body 内包含 InnerPanel——Outer 的缓存子树携带 Inner 的订阅。
#[derive(DslComponent)]
struct OuterPanel {
    #[watch]
    value: Signal<u32>,
}

impl OuterPanel {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        bump_a();
        ui!(build {
            <Column>
                <Text value={format!("O:{}", self.value.get())} />
                <InnerPanel value={self.inner_source()} />
            </Column>
        })
    }

    // Outer 不拥有 Inner 的 signal（由测试在 props 之外注入）：为让嵌套用例使用
    // 独立 b 源，这里经全局槽传递，见 nested fixture。
}

/// A derive parent that consumes caller-provided children. Its retained entry must retain the
/// materialized child slots rather than retaining the `ui!` closure that produced them.
#[derive(DslComponent)]
struct SlotHost {
    #[watch]
    value: Signal<u32>,
}

impl SlotHost {
    fn view<A>(&self, build: &mut ViewBuild<A>, children: Body<A>) -> ViewResult<ViewOutput<A>> {
        bump_a();
        let node = build.container(UiNode::new(NodeKind::Column), children)?;
        build.finish(
            Body::new(vec![ViewChild::view_node(node)], Vec::new()),
            ViewSite::new(file!(), line!(), column!()),
        )
    }
}

// 嵌套用例的 Inner signal 槽：Outer 构造时从这里取（thread_local 保存 Rc 句柄）。
thread_local! {
    static NESTED_INNER: std::cell::RefCell<Option<Signal<u32>>> =
        const { std::cell::RefCell::new(None) };
}

fn render_root(
    build: &mut ViewBuild<Action>,
    a: &Signal<u32>,
    b: &Signal<u32>,
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <PanelA value={a.clone()} />
            <PanelB value={b.clone()} />
        </Column>
    })
}

fn render_nested(
    build: &mut ViewBuild<Action>,
    outer: &Signal<u32>,
    inner: &Signal<u32>,
) -> ViewResult<ViewOutput<Action>> {
    NESTED_INNER.with(|slot| *slot.borrow_mut() = Some(inner.clone()));
    ui!(build {
        <Column>
            <OuterPanel value={outer.clone()} />
        </Column>
    })
}

fn render_for_panels(
    build: &mut ViewBuild<Action>,
    items: &[(u32, Signal<u32>)],
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <For each={items} key={item.0}>
                {|item|
                    <ForPanel value={item.1.clone()} />
                }
            </For>
        </Column>
    })
}

fn render_slot_host(
    build: &mut ViewBuild<Action>,
    parent: &Signal<u32>,
    child: &Signal<u32>,
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <SlotHost value={parent.clone()}>
            <InnerPanel value={child.clone()} />
        </SlotHost>
    })
}

impl OuterPanel {
    fn inner_source(&self) -> Signal<u32> {
        NESTED_INNER.with(|slot| {
            slot.borrow()
                .clone()
                .expect("nested fixture installs the inner signal before render")
        })
    }
}

fn empty_frame() -> UiFrame {
    UiFrame {
        viewport: Viewport {
            width: 100.0,
            height: 100.0,
        },
        commands: Vec::new(),
        hit_regions: Vec::new(),
        scroll_bounds: Vec::new(),
    }
}

struct Fixture {
    coordinator: FrameCoordinator<Action>,
    a: Signal<u32>,
    b: Signal<u32>,
}

impl Fixture {
    fn new(a: u32, b: u32) -> Self {
        Self {
            coordinator: FrameCoordinator::new(),
            a: Signal::new(a),
            b: Signal::new(b),
        }
    }

    /// 用指定 dirty 集发布一帧；signal 写入后由调用方取走 dirty。
    fn publish(&mut self, dirty: BTreeSet<SemanticKey>, retained_enabled: bool) {
        let mut build = self
            .coordinator
            .begin_build_for_frame(dirty, retained_enabled);
        let root = render_root(&mut build, &self.a, &self.b).expect("root view");
        let prepared = self.coordinator.prepare(root).expect("candidate frame");
        let resolved = prepared
            .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
            .expect("resolved frame");
        self.coordinator.commit(resolved);
    }

    /// 不运行 root projection，直接以 active retained 坐标重入一批 dirty component。
    fn publish_retained_dirty(&mut self, dirty: BTreeSet<SemanticKey>) {
        let prepared = self
            .coordinator
            .prepare_retained_dirty(dirty)
            .expect("retained candidate")
            .expect("dirty component has an active retained coordinate");
        let resolved = prepared
            .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
            .expect("resolved retained candidate");
        self.coordinator.commit(resolved);
    }

    /// 写入 A 的 signal 并取走 dirty 集。
    fn touch_a(&mut self, value: u32) -> BTreeSet<SemanticKey> {
        self.a.set(value);
        self.coordinator.runtime().take_dirty()
    }

    fn touch_b(&mut self, value: u32) -> BTreeSet<SemanticKey> {
        self.b.set(value);
        self.coordinator.runtime().take_dirty()
    }

    fn tree_debug(&self) -> String {
        format!(
            "{:?}",
            self.coordinator
                .active()
                .expect("active frame")
                .tree()
                .root()
        )
    }
}

#[test]
fn retained_frame_renders_only_the_dirty_component() {
    let mut fixture = Fixture::new(1, 1);
    RENDER_A.with(|c| c.set(0));
    RENDER_B.with(|c| c.set(0));

    // 首帧：全量渲染并记录缓存。
    fixture.publish(BTreeSet::new(), true);
    assert_eq!(renders_a(), 1);
    assert_eq!(renders_b(), 1);

    // A 的 signal 变化：只重跑 A，B 命中缓存（纯 SignalId + 脏集判定）。
    let dirty = fixture.touch_a(5);
    assert_eq!(dirty.len(), 1, "只有一个组件的订阅被标脏");
    assert_eq!(
        fixture
            .coordinator
            .outermost_dirty_retained_roots(&dirty)
            .len(),
        1,
        "dirty key resolves directly to one retained coordinate"
    );
    fixture.publish(dirty, true);
    assert_eq!(renders_a(), 2);
    assert_eq!(renders_b(), 1, "不相关组件命中缓存");

    // B 的 signal 变化：只重跑 B，A 命中缓存。
    let dirty = fixture.touch_b(7);
    assert_eq!(dirty.len(), 1);
    fixture.publish(dirty, true);
    assert_eq!(renders_a(), 2, "不相关组件命中缓存");
    assert_eq!(renders_b(), 2);

    // 相同值写入被相等性短路：无 dirty，无重建。
    let dirty = fixture.touch_a(5);
    assert!(dirty.is_empty());
    fixture.publish(BTreeSet::new(), true);
    assert_eq!(renders_a(), 2);
    assert_eq!(renders_b(), 2);
}

#[test]
fn dirty_coordinate_reenters_without_running_the_root_projection() {
    let mut fixture = Fixture::new(1, 1);
    RENDER_A.with(|c| c.set(0));
    RENDER_B.with(|c| c.set(0));
    fixture.publish(BTreeSet::new(), true);

    let before = fixture.tree_debug();
    let dirty = fixture.touch_a(9);
    fixture.publish_retained_dirty(dirty);

    assert_eq!(
        renders_a(),
        2,
        "only the retained root responsible for A re-entered"
    );
    assert_eq!(
        renders_b(),
        1,
        "the clean sibling stayed on its shared subtree"
    );
    assert_ne!(
        fixture.tree_debug(),
        before,
        "the spliced output contains A's new value"
    );

    // A direct commit must keep B's unvisited retained entry alive for the next independent
    // dirty coordinate; otherwise this would fall back to a rooted projection.
    let dirty = fixture.touch_b(4);
    fixture.publish_retained_dirty(dirty);
    assert_eq!(renders_a(), 2);
    assert_eq!(
        renders_b(),
        2,
        "clean sibling cache survived the previous direct commit"
    );
}

#[test]
fn for_item_retained_root_uses_its_resolved_watch_coordinate() {
    let mut coordinator = FrameCoordinator::<Action>::new();
    let first = Signal::new(1_u32);
    let second = Signal::new(2_u32);
    let items = vec![(10_u32, first.clone()), (20_u32, second.clone())];
    RENDER_A.with(|c| c.set(0));

    let mut build = coordinator.begin_build_for_frame(BTreeSet::new(), true);
    let root = render_for_panels(&mut build, &items).expect("for root");
    let resolved = coordinator
        .prepare(root)
        .expect("for candidate")
        .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
        .expect("for frame");
    coordinator.commit(resolved);
    assert_eq!(renders_a(), 2);
    let keys_before = coordinator
        .active()
        .expect("active for frame")
        .tree()
        .keys()
        .to_vec();

    second.set(3);
    let dirty = coordinator.runtime().take_dirty();
    let prepared = coordinator
        .prepare_retained_dirty(dirty)
        .expect("for retained candidate")
        .expect("For-decorated retained root keeps a direct coordinate");
    let resolved = prepared
        .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
        .expect("for retained frame");
    coordinator.commit(resolved);
    assert_eq!(renders_a(), 3, "only the dirty keyed item re-entered");
    assert_eq!(
        coordinator
            .active()
            .expect("active direct frame")
            .tree()
            .keys(),
        keys_before,
        "direct re-entry preserves the For item's business-key identity shell"
    );
}

#[test]
fn retained_frames_match_full_renders_tree_for_tree() {
    let mut retained = Fixture::new(1, 1);
    let mut full = Fixture::new(1, 1);

    retained.publish(BTreeSet::new(), true);
    full.publish(BTreeSet::new(), false);
    assert_eq!(retained.tree_debug(), full.tree_debug());

    let dirty = retained.touch_a(5);
    full.a.set(5);
    let _ = full.coordinator.runtime().take_dirty();
    retained.publish(dirty, true);
    full.publish(BTreeSet::new(), false);
    assert_eq!(retained.tree_debug(), full.tree_debug());

    let dirty_b = retained.touch_b(9);
    let dirty_a = retained.touch_a(11);
    full.b.set(9);
    full.a.set(11);
    let _ = full.coordinator.runtime().take_dirty();
    let merged: BTreeSet<SemanticKey> = dirty_b.union(&dirty_a).cloned().collect();
    retained.publish(merged, true);
    full.publish(BTreeSet::new(), false);
    assert_eq!(retained.tree_debug(), full.tree_debug());
}

#[test]
fn disabled_frames_refresh_watch_keys_and_do_not_pollute_cache() {
    let mut fixture = Fixture::new(1, 1);

    // retained 帧记录缓存。
    fixture.publish(BTreeSet::new(), true);

    // 关闭 retained 的全量帧：watch keys 仍随提交刷新。
    fixture.publish(BTreeSet::new(), false);

    // 重新启用且无脏：两个组件都应命中（树结构未变，key 映射未陈旧）。
    RENDER_A.with(|c| c.set(0));
    RENDER_B.with(|c| c.set(0));
    fixture.publish(BTreeSet::new(), true);
    fixture.publish(BTreeSet::new(), true);
    assert_eq!(renders_a(), 0, "两次干净帧全部命中缓存");
    assert_eq!(renders_b(), 0);

    // signal 变化仍能穿透缓存（key 映射在禁用帧后被正确刷新）。
    let dirty = fixture.touch_a(3);
    assert_eq!(dirty.len(), 1);
    fixture.publish(dirty, true);
    assert_eq!(renders_a(), 1);
}

#[test]
fn rejected_candidate_discards_pending_memo_entries() {
    let mut fixture = Fixture::new(1, 1);
    fixture.publish(BTreeSet::new(), true);
    let before = fixture.tree_debug();

    // 构建一个会在 resolve 阶段失败的候选：记录了新缓存但从未提交。
    let dirty = fixture.touch_a(5);
    let mut build = fixture
        .coordinator
        .begin_build_for_frame(dirty.clone(), true);
    let root = render_root(&mut build, &fixture.a, &fixture.b).expect("root view");
    let prepared = fixture.coordinator.prepare(root).expect("candidate frame");
    let rejected = prepared.resolve(|_| Err::<UiFrame, String>("layout failed".to_owned()));
    assert!(rejected.is_err());
    // 宿主在 frame_rejected 时同样恢复 dirty（app-runtime 语义），这里保持一致。
    fixture.coordinator.abort_component_transaction();
    fixture.coordinator.runtime().restore_dirty(dirty);

    // 旧 active 树保持不变。
    assert_eq!(fixture.tree_debug(), before);

    // 恢复的 dirty 驱动下一次重建：失败候选的缓存条目被丢弃，A 重新渲染。
    let dirty = fixture.coordinator.runtime().take_dirty();
    assert_eq!(dirty.len(), 1, "失败候选消费过的脏标被恢复");
    RENDER_A.with(|c| c.set(0));
    RENDER_B.with(|c| c.set(0));
    fixture.publish(dirty, true);
    assert_eq!(renders_a(), 1, "失败候选未留下缓存");
    assert_eq!(renders_b(), 0, "未受影响的组件仍命中旧 active 缓存");
    assert_ne!(fixture.tree_debug(), before, "A 的新值已生效");
}

#[test]
fn nested_retained_inner_stays_frozen_when_outer_renders() {
    let mut coordinator = FrameCoordinator::<Action>::new();
    let outer_source = Signal::new(1_u32);
    let inner_source = Signal::new(1_u32);

    let publish = |coordinator: &mut FrameCoordinator<Action>,
                   outer: &Signal<u32>,
                   inner: &Signal<u32>,
                   dirty: BTreeSet<SemanticKey>,
                   enabled: bool| {
        let mut build = coordinator.begin_build_for_frame(dirty, enabled);
        let root = render_nested(&mut build, outer, inner).expect("nested root");
        let prepared = coordinator.prepare(root).expect("candidate frame");
        let resolved = prepared
            .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
            .expect("resolved frame");
        coordinator.commit(resolved);
    };

    // 首帧：Outer 与 Inner 各渲染一次并记录。
    RENDER_A.with(|c| c.set(0));
    RENDER_INNER.with(|c| c.set(0));
    publish(
        &mut coordinator,
        &outer_source,
        &inner_source,
        BTreeSet::new(),
        true,
    );
    assert_eq!(renders_a(), 1);
    assert_eq!(renders_inner(), 1);

    // Outer 的 signal 变化：Outer miss 重渲染，Inner 的边未脏 → 命中冻结。
    outer_source.set(2);
    let dirty = coordinator.runtime().take_dirty();
    let prepared = coordinator
        .prepare_retained_dirty(dirty)
        .expect("outer retained candidate")
        .expect("outer dirty coordinate");
    let resolved = prepared
        .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
        .expect("resolved outer retained candidate");
    coordinator.commit(resolved);
    assert_eq!(renders_a(), 2, "Outer 的订阅脏 → 重渲染");
    assert_eq!(
        renders_inner(),
        1,
        "嵌套内层命中缓存（父也未运行 root projection）"
    );

    // Inner 的 signal 变化：选择最内层 retained coordinate，直接替换 Inner；Outer
    // 的共享父树不执行 view。
    inner_source.set(3);
    let dirty = coordinator.runtime().take_dirty();
    assert_eq!(
        coordinator.outermost_dirty_retained_roots(&dirty).len(),
        1,
        "nested dirty scope resolves to the innermost retained root"
    );
    let prepared = coordinator
        .prepare_retained_dirty(dirty)
        .expect("inner retained candidate")
        .expect("nested dirty coordinate");
    let resolved = prepared
        .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
        .expect("resolved inner retained candidate");
    coordinator.commit(resolved);
    assert_eq!(renders_a(), 2, "父级共享子树不因子级边脏而重跑");
    assert_eq!(renders_inner(), 2, "内层重渲染");
}

#[test]
fn retained_children_restore_parent_body_without_retaining_the_ui_closure() {
    let mut coordinator = FrameCoordinator::<Action>::new();
    let parent = Signal::new(1_u32);
    let child = Signal::new(1_u32);
    RENDER_A.with(|count| count.set(0));
    RENDER_INNER.with(|count| count.set(0));

    let publish = |coordinator: &mut FrameCoordinator<Action>, dirty: BTreeSet<SemanticKey>| {
        let mut build = coordinator.begin_build_for_frame(dirty, true);
        let root = render_slot_host(&mut build, &parent, &child).expect("slot host root");
        let resolved = coordinator
            .prepare(root)
            .expect("slot host candidate")
            .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
            .expect("slot host frame");
        coordinator.commit(resolved);
    };

    publish(&mut coordinator, BTreeSet::new());
    assert_eq!(renders_a(), 1);
    assert_eq!(renders_inner(), 1);

    // The parent re-enters from a retained Body snapshot. The original `ui!` child closure has
    // already been consumed, so this catches accidental cross-frame closure retention.
    parent.set(2);
    let dirty = coordinator.runtime().take_dirty();
    let prepared = coordinator
        .prepare_retained_dirty(dirty)
        .expect("parent retained candidate")
        .expect("parent retained coordinate");
    let resolved = prepared
        .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
        .expect("parent retained frame");
    coordinator.commit(resolved);
    assert_eq!(renders_a(), 2);
    assert_eq!(renders_inner(), 1, "child node was restored by Rc identity");

    child.set(3);
    let dirty = coordinator.runtime().take_dirty();
    let prepared = coordinator
        .prepare_retained_dirty(dirty)
        .expect("child retained candidate")
        .expect("child retained coordinate");
    let resolved = prepared
        .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
        .expect("child retained frame");
    coordinator.commit(resolved);
    assert_eq!(
        renders_a(),
        2,
        "child dirty edge does not re-enter the parent"
    );
    assert_eq!(renders_inner(), 2);
}
