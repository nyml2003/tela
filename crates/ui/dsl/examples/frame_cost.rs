//! Lightweight, dependency-free measurement entry point for the DSL frame path.
//!
//! Run with:
//! `cargo run --release -p tela-ui-dsl --example frame_cost -- --nodes 1000 --iterations 200`
//!
//! This intentionally reports observations instead of enforcing a timing budget. It exercises
//! actual `ui!` lowering, `For` key resolution, `@watch` plan reconciliation, dirty marking, and
//! the optional Host invalidator protocol without claiming a machine-independent performance
//! target.

use std::{
    cell::Cell,
    collections::BTreeSet,
    env,
    hint::black_box,
    rc::Rc,
    time::{Duration, Instant},
};

use tela_contract::{SemanticKey, UiFrame, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Body, DslComponent, FrameCoordinator, FrameInvalidator, Signal, ViewBuild, ViewOutput,
    ViewResult, ui,
};

#[allow(dead_code)]
#[derive(Clone)]
struct BenchmarkAction;

struct BenchmarkItem {
    id: u32,
    label: String,
}

/// 记忆化基准行：每行订阅自己的 row signal（label 并入数据，满足 watch-only 契约）。
#[derive(Clone, PartialEq)]
struct RowData {
    #[allow(dead_code)]
    label: String,
    value: u32,
}

struct BenchmarkRow {
    id: u32,
    row: Signal<RowData>,
}

thread_local! {
    static ROW_RENDERS: Cell<usize> = const { Cell::new(0) };
}

/// retained 行组件（默认，无注解）：入边无脏 → 不重求值。
#[derive(DslComponent)]
struct WatchedRow {
    #[watch]
    row: Signal<RowData>,
}

impl WatchedRow {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        ROW_RENDERS.with(|count| count.set(count.get() + 1));
        let text = self
            .row
            .with(|row| format!("{}={}", row.label, row.value));
        ui!(build {
            <Frame>
                <Text value={text} />
            </Frame>
        })
    }
}

/// 根订阅 version signal 的组件：使 `version.set` 标脏根 key 并唤醒宿主
/// （`Signal` 不隐式追踪读取，订阅必须显式声明）。
#[derive(DslComponent)]
struct WatchedVersion {
    #[watch]
    version: Signal<u64>,
}

impl WatchedVersion {
    fn view<A>(&self, build: &mut ViewBuild<A>, _children: Body<A>) -> ViewResult<ViewOutput<A>> {
        ui!(build {
            <Text value={format!("version={}", self.version.get())} />
        })
    }
}

struct CountingInvalidator {
    requests: Cell<u64>,
}

impl FrameInvalidator for CountingInvalidator {
    fn request_frame(&self) {
        self.requests.set(self.requests.get() + 1);
    }
}

fn render(
    build: &mut ViewBuild<BenchmarkAction>,
    version: &Signal<u64>,
    items: &[BenchmarkItem],
) -> ViewResult<ViewOutput<BenchmarkAction>> {
    ui!(build {
        <Column>
            <WatchedVersion version={version.clone()} />
            <For each={items} key={item.id}>
                {|item|
                    <Frame>
                        <Text value={item.label.clone()} />
                    </Frame>
                }
            </For>
        </Column>
    })
}

fn render_memoized(
    build: &mut ViewBuild<BenchmarkAction>,
    version: &Signal<u64>,
    rows: &[BenchmarkRow],
) -> ViewResult<ViewOutput<BenchmarkAction>> {
    ui!(build {
        <Column>
            <WatchedVersion version={version.clone()} />
            <For each={rows} key={row.id}>
                {|row|
                    <Frame>
                        <WatchedRow row={row.row.clone()} />
                    </Frame>
                }
            </For>
        </Column>
    })
}

fn empty_frame() -> UiFrame {
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

fn publish(coordinator: &mut FrameCoordinator<BenchmarkAction>, root: ViewOutput<BenchmarkAction>) {
    let prepared = coordinator
        .prepare(root)
        .expect("benchmark tree must be valid");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("benchmark resolve must be infallible");
    coordinator.commit(resolved);
}

/// 只更新行的 value 字段（label 恒定），保持脏标语义与旧版一致。
fn set_row_value(row: &BenchmarkRow, value: u32) {
    let label = row.row.with(|data| data.label.clone());
    row.row.set(RowData { label, value });
}

/// 记忆化路径：以本帧 dirty 集构建、准备并提交；`memo_enabled` 控制是否启用缓存。
fn publish_memoized(
    coordinator: &mut FrameCoordinator<BenchmarkAction>,
    dirty: BTreeSet<SemanticKey>,
    memo_enabled: bool,
    version: &Signal<u64>,
    rows: &[BenchmarkRow],
) {
    let mut build = coordinator.begin_build_for_frame(dirty, memo_enabled);
    let root = render_memoized(&mut build, version, rows).expect("memoized benchmark view");
    drop(build);
    let prepared = coordinator
        .prepare(root)
        .expect("benchmark tree must be valid");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("benchmark resolve must be infallible");
    coordinator.commit(resolved);
}

fn duration_per_iteration(duration: Duration, iterations: usize) -> f64 {
    duration.as_secs_f64() * 1_000_000.0 / iterations as f64
}

fn parse_config() -> (usize, usize) {
    let mut nodes = 1_000_usize;
    let mut iterations = 200_usize;
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let target = match argument.as_str() {
            "--nodes" => &mut nodes,
            "--iterations" => &mut iterations,
            "--help" | "-h" => {
                println!("usage: frame_cost [--nodes N] [--iterations N]");
                std::process::exit(0);
            }
            _ => panic!("unknown argument `{argument}`; use --help"),
        };
        let value = arguments
            .next()
            .unwrap_or_else(|| panic!("{argument} requires a positive integer"));
        *target = value
            .parse()
            .unwrap_or_else(|_| panic!("{argument} must be a positive integer"));
        assert!(*target > 0, "{argument} must be greater than zero");
    }
    (nodes, iterations)
}

fn main() {
    let (nodes, iterations) = parse_config();
    let items = (0..nodes)
        .map(|id| BenchmarkItem {
            id: u32::try_from(id).expect("node count must fit the DSL demo item id"),
            label: format!("item-{id}"),
        })
        .collect::<Vec<_>>();
    let version = Signal::new(0_u64);
    let mut coordinator = FrameCoordinator::new();

    let mut initial_build = coordinator.begin_build();
    publish(
        &mut coordinator,
        render(&mut initial_build, &version, &items).expect("initial benchmark view"),
    );

    let invalidator = Rc::new(CountingInvalidator {
        requests: Cell::new(0),
    });
    let invalidator_port: Rc<dyn FrameInvalidator> = invalidator.clone();
    coordinator.runtime().set_invalidator(invalidator_port);

    let mut dirty_cost = Duration::ZERO;
    let mut rebuild_cost = Duration::ZERO;
    for iteration in 1..=iterations {
        let dirty_started = Instant::now();
        version.set(u64::try_from(iteration).expect("iteration must fit u64"));
        coordinator.runtime().begin_frame();
        let dirty = coordinator.runtime().take_dirty();
        dirty_cost += dirty_started.elapsed();
        assert_eq!(dirty.len(), 1, "one watched root should become dirty");

        let rebuild_started = Instant::now();
        let mut build = coordinator.begin_build();
        let root = render(&mut build, &version, &items).expect("benchmark view");
        publish(&mut coordinator, root);
        rebuild_cost += rebuild_started.elapsed();
        black_box(
            coordinator
                .active()
                .expect("published frame")
                .tree()
                .keys()
                .len(),
        );
    }

    assert_eq!(
        invalidator.requests.get(),
        u64::try_from(iterations).expect("iteration count must fit u64"),
        "one Signal update should request one Host frame",
    );

    // 相同值写入被相等性短路：不标脏、不唤醒宿主。
    let requests_before_noop = invalidator.requests.get();
    for _ in 0..iterations {
        version.set(u64::try_from(iterations).expect("iteration must fit u64"));
    }
    assert_eq!(
        invalidator.requests.get(),
        requests_before_noop,
        "same-value writes must be short-circuited",
    );

    // 组件路径对照：同样的 WatchedRow 树，但每帧关闭记忆化（全部行重渲染）。
    let rows = (0..nodes)
        .map(|id| BenchmarkRow {
            id: u32::try_from(id).expect("node count must fit the DSL demo item id"),
            row: Signal::new(RowData {
                label: format!("item-{id}"),
                value: 0,
            }),
        })
        .collect::<Vec<_>>();
    let mut component_coordinator = FrameCoordinator::new();
    publish_memoized(
        &mut component_coordinator,
        BTreeSet::new(),
        false,
        &version,
        &rows,
    );
    let mut component_cost = Duration::ZERO;
    for iteration in 1..=iterations {
        let touched = rows
            .get(iteration % rows.len())
            .expect("row index must exist");
        set_row_value(touched, u32::try_from(iteration).expect("iteration must fit u32"));
        component_coordinator.runtime().begin_frame();
        let dirty = component_coordinator.runtime().take_dirty();
        assert_eq!(dirty.len(), 1, "one watched row should become dirty");

        let component_started = Instant::now();
        publish_memoized(
            &mut component_coordinator,
            dirty,
            false,
            &version,
            &rows,
        );
        component_cost += component_started.elapsed();
        black_box(
            component_coordinator
                .active()
                .expect("published frame")
                .tree()
                .keys()
                .len(),
        );
    }

    // 记忆化路径：每行一个 #[memo] 组件订阅自己的 signal；每次只改一行，
    // 其余行命中缓存跳过 render。
    let mut memo_coordinator = FrameCoordinator::new();
    publish_memoized(
        &mut memo_coordinator,
        BTreeSet::new(),
        true,
        &version,
        &rows,
    );
    let mut memoized_cost = Duration::ZERO;
    ROW_RENDERS.with(|count| count.set(0));
    for iteration in 1..=iterations {
        let touched = rows
            .get(iteration % rows.len())
            .expect("row index must exist");
        // 偏移取值：避免与对照循环相同，否则被 Signal 相等性短路。
        let fresh = u32::try_from(iterations + iteration).expect("iteration must fit u32");
        set_row_value(touched, fresh);
        memo_coordinator.runtime().begin_frame();
        let dirty = memo_coordinator.runtime().take_dirty();
        assert_eq!(dirty.len(), 1, "one watched row should become dirty");

        let memoized_started = Instant::now();
        publish_memoized(&mut memo_coordinator, dirty, true, &version, &rows);
        memoized_cost += memoized_started.elapsed();
        black_box(
            memo_coordinator
                .active()
                .expect("published frame")
                .tree()
                .keys()
                .len(),
        );
    }
    let row_renders = ROW_RENDERS.with(Cell::get);
    assert_eq!(
        row_renders, iterations,
        "memoized frames must re-render only the touched row",
    );

    println!("nodes={nodes} iterations={iterations}");
    println!(
        "dirty_and_schedule_us_per_iteration={:.3}",
        duration_per_iteration(dirty_cost, iterations)
    );
    println!(
        "build_prepare_reconcile_commit_us_per_iteration={:.3}",
        duration_per_iteration(rebuild_cost, iterations)
    );
    println!(
        "component_rebuild_us_per_iteration={:.3}",
        duration_per_iteration(component_cost, iterations)
    );
    println!(
        "memoized_rebuild_us_per_iteration={:.3}",
        duration_per_iteration(memoized_cost, iterations)
    );
}
