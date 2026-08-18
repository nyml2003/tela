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
    env,
    hint::black_box,
    rc::Rc,
    time::{Duration, Instant},
};

use tela_contract::{UiFrame, Viewport};
use tela_ui_dsl::{
    FrameCoordinator, FrameInvalidator, Signal, ViewBuild, ViewOutput, ViewResult, ui,
};

#[allow(dead_code)]
#[derive(Clone)]
struct BenchmarkAction;

struct BenchmarkItem {
    id: u32,
    label: String,
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
        @watch(current_version, &version);

        <Column>
            <Text value={format!("version={}", current_version.get())} />
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
    println!("nodes={nodes} iterations={iterations}");
    println!(
        "dirty_and_schedule_us_per_iteration={:.3}",
        duration_per_iteration(dirty_cost, iterations)
    );
    println!(
        "build_prepare_reconcile_commit_us_per_iteration={:.3}",
        duration_per_iteration(rebuild_cost, iterations)
    );
}
