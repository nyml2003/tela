//! `#[memo]` 组件记忆化的一致性与粒度集成测试。
//!
//! 对齐 `m5_update.rs` 的 Full/Dirty 一致性风格：同一 signal 变更序列下，
//! 记忆化帧与全量渲染帧必须产出完全相同的树，且 render 只重跑订阅被标脏的组件。

use std::{cell::Cell, collections::BTreeSet};

use tela_contract::{SemanticKey, UiFrame, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, DslComponent, FrameCoordinator, Signal, ViewBuild, ViewOutput, ViewResult, ui,
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
}

fn bump_a() {
    RENDER_A.with(|count| count.set(count.get() + 1));
}

fn bump_b() {
    RENDER_B.with(|count| count.set(count.get() + 1));
}

fn renders_a() -> usize {
    RENDER_A.with(Cell::get)
}

fn renders_b() -> usize {
    RENDER_B.with(Cell::get)
}

/// 面板 A：订阅 `shared`，`#[memo]` 记忆化。
#[derive(DslComponent)]
#[memo]
struct PanelA {
    #[watch]
    value: Signal<u32>,
    label: u32,
}

impl PanelA {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        bump_a();
        ui!(build {
            <Text value={format!("A{}:{}", self.label, self.value.get())} />
        })
    }
}

/// 面板 B：独立 signal，与 A 互为"不相关兄弟组件"。
#[derive(DslComponent)]
#[memo]
struct PanelB {
    #[watch]
    value: Signal<u32>,
    label: u32,
}

impl PanelB {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        bump_b();
        ui!(build {
            <Text value={format!("B{}:{}", self.label, self.value.get())} />
        })
    }
}

fn render_root_with_labels(
    build: &mut ViewBuild<Action>,
    a: &Signal<u32>,
    b: &Signal<u32>,
    label_a: u32,
) -> ViewResult<ViewOutput<Action>> {
    ui!(build {
        <Column>
            <PanelA value={a.clone()} label={label_a} />
            <PanelB value={b.clone()} label={2_u32} />
        </Column>
    })
}

fn render_root(
    build: &mut ViewBuild<Action>,
    a: &Signal<u32>,
    b: &Signal<u32>,
) -> ViewResult<ViewOutput<Action>> {
    render_root_with_labels(build, a, b, 1)
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
    fn publish(&mut self, dirty: BTreeSet<SemanticKey>, memo_enabled: bool) {
        let mut build = self
            .coordinator
            .begin_build_for_frame(dirty, memo_enabled);
        let root = render_root(&mut build, &self.a, &self.b).expect("root view");
        let prepared = self.coordinator.prepare(root).expect("candidate frame");
        let resolved = prepared
            .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
            .expect("resolved frame");
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
        format!("{:?}", self.coordinator.active().expect("active frame").tree().root())
    }
}

#[test]
fn memoized_frame_renders_only_the_dirty_component() {
    let mut fixture = Fixture::new(1, 1);
    RENDER_A.with(|c| c.set(0));
    RENDER_B.with(|c| c.set(0));

    // 首帧：全量渲染并记录缓存。
    fixture.publish(BTreeSet::new(), true);
    assert_eq!(renders_a(), 1);
    assert_eq!(renders_b(), 1);

    // A 的 signal 变化：只重跑 A，B 命中缓存。
    let dirty = fixture.touch_a(5);
    assert_eq!(dirty.len(), 1, "只有一个组件的订阅被标脏");
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
fn memoized_frames_match_full_renders_tree_for_tree() {
    let mut memoized = Fixture::new(1, 1);
    let mut full = Fixture::new(1, 1);

    memoized.publish(BTreeSet::new(), true);
    full.publish(BTreeSet::new(), false);
    assert_eq!(memoized.tree_debug(), full.tree_debug());

    let dirty = memoized.touch_a(5);
    full.a.set(5);
    let _ = full.coordinator.runtime().take_dirty();
    memoized.publish(dirty, true);
    full.publish(BTreeSet::new(), false);
    assert_eq!(memoized.tree_debug(), full.tree_debug());

    let dirty = memoized.touch_b(9);
    let dirty_b = memoized.touch_a(11);
    full.b.set(9);
    full.a.set(11);
    let _ = full.coordinator.runtime().take_dirty();
    let merged: BTreeSet<SemanticKey> = dirty.union(&dirty_b).cloned().collect();
    memoized.publish(merged, true);
    full.publish(BTreeSet::new(), false);
    assert_eq!(memoized.tree_debug(), full.tree_debug());
}

#[test]
fn disabled_frames_refresh_watch_keys_and_do_not_pollute_cache() {
    let mut fixture = Fixture::new(1, 1);

    // 记忆化帧记录缓存。
    fixture.publish(BTreeSet::new(), true);

    // 关闭记忆化的全量帧：watch keys 仍随提交刷新。
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
fn props_change_invalidates_the_cache_without_any_dirty_key() {
    let mut fixture = Fixture::new(1, 1);
    fixture.publish(BTreeSet::new(), true);

    // label 是普通 props：只要变化，即使没有任何 signal 脏标也必须重渲染。
    // 这里通过重建 root（label 随构造传入）验证指纹比较路径。
    RENDER_A.with(|c| c.set(0));
    RENDER_B.with(|c| c.set(0));
    let mut build = fixture
        .coordinator
        .begin_build_for_frame(BTreeSet::new(), true);
    let root = render_root_with_labels(&mut build, &fixture.a, &fixture.b, 9).expect("root view");
    let prepared = fixture.coordinator.prepare(root).expect("candidate frame");
    let resolved = prepared
        .resolve(|_| Ok::<UiFrame, ()>(empty_frame()))
        .expect("resolved frame");
    fixture.coordinator.commit(resolved);
    assert_eq!(renders_a(), 1, "props 变化穿透缓存");
    assert_eq!(renders_b(), 0, "props 未变的组件仍命中");
}
