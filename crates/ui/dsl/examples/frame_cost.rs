//! Small, dependency-free frame-cost probe for the v3 DSL surface.
//!
//! Run with:
//! `cargo run --release -p tela-ui-dsl --example frame_cost -- --nodes 1000 --iterations 200`

use std::{
    env,
    time::{Duration, Instant},
};

use tela_contract::{RenderPlan, UiFrame, Viewport};
use tela_ui_dsl::prelude::*;
use tela_ui_dsl::{
    Children, DirtySet, DslComponent, ForContext, FrameCoordinator, Signal, ViewBuild, ViewOutput,
    ViewResult, signal, ui,
};

#[derive(Clone)]
struct Item {
    id: u32,
    label: String,
}

#[derive(DslComponent)]
struct VersionText {
    #[watch]
    version: Signal<u64>,
}

impl VersionText {
    fn view<A: 'static>(
        &self,
        build: &mut ViewBuild<A>,
        _children: &Children<'_, A>,
    ) -> ViewResult<ViewOutput<A>> {
        ui!(build {
            <Text value={format!("version={}", self.version.get())} />
        })
    }
}

fn item_key(context: ForContext<Item>) -> String {
    context.item.id.to_string()
}

fn render_item(build: &mut ViewBuild<()>, context: ForContext<Item>) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <View>
            <Text value={context.item.label} />
        </View>
    })
}

fn render(
    build: &mut ViewBuild<()>,
    version: &Signal<u64>,
    items: &[Item],
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column>
            <VersionText version={version.clone()} />
            <For each={items} key={item_key} row={render_item} />
        </Column>
    })
}

fn empty_frame() -> RenderPlan {
    RenderPlan::from_flat_frame(UiFrame {
        viewport: Viewport {
            width: 1280.0,
            height: 720.0,
        },
        commands: Vec::new(),
        hit_regions: Vec::new(),
        scroll_bounds: Vec::new(),
    })
}

fn publish(
    coordinator: &mut FrameCoordinator<()>,
    dirty: DirtySet,
    version: &Signal<u64>,
    items: &[Item],
) {
    let mut build = coordinator.begin_build_for_frame(dirty, true);
    let root = render(&mut build, version, items).expect("benchmark view");
    let resolved = coordinator
        .prepare(root)
        .expect("benchmark tree")
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("benchmark resolve");
    coordinator
        .commit(resolved)
        .expect("current benchmark frame");
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
        .map(|id| Item {
            id: u32::try_from(id).expect("node count must fit u32"),
            label: format!("item-{id}"),
        })
        .collect::<Vec<_>>();
    let (version_writer, version) = signal(0_u64);
    let mut coordinator = FrameCoordinator::new();

    publish(&mut coordinator, DirtySet::default(), &version, &items);

    let mut total = Duration::ZERO;
    for iteration in 1..=iterations {
        coordinator.runtime().begin_frame();
        let started = Instant::now();
        version_writer.set(u64::try_from(iteration).expect("iteration fits u64"));
        let dirty = coordinator.runtime().take_dirty();
        publish(&mut coordinator, dirty, &version, &items);
        total += started.elapsed();
    }

    println!(
        "nodes={nodes} iterations={iterations} avg_us={:.2}",
        total.as_secs_f64() * 1_000_000.0 / iterations as f64
    );
}
