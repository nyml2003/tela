//! Direct `Signal` sources owned by transparent `Show` / `For` components.
//!
//! These cases are intentionally about lifecycle rather than visual bindings: neither
//! structural component owns a physical node, so its source must survive an empty collection,
//! be retried transactionally, and disappear exactly when the structure is unmounted.

use tela_contract::{ContentConcern, RenderPlan, SemanticKey, UiFrame, Viewport};
use tela_ui_dsl::prelude::{Column, For, Row, Show, Switch, Text};
use tela_ui_dsl::{
    DirtySet, ForContext, FrameCoordinator, ShowContext, Signal, SwitchContext, ViewBuild,
    ViewOutput, ViewResult, signal, ui,
};

#[derive(Clone)]
struct RowData {
    id: &'static str,
    label: &'static str,
}

fn row_key(context: ForContext<RowData>) -> String {
    context.item.id.to_owned()
}

fn render_row(
    build: &mut ViewBuild<()>,
    context: ForContext<RowData>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Row>
            <Text value={format!("{}:{}", context.index, context.item.label)} />
        </Row>
    })
}

fn direct_for_root(
    build: &mut ViewBuild<()>,
    rows: Signal<Vec<RowData>>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column key={"direct-for-root"}>
            <For each={rows} key={row_key} row={render_row} />
        </Column>
    })
}

fn empty_frame() -> RenderPlan {
    RenderPlan::from_flat_frame(UiFrame {
        viewport: Viewport {
            width: 320.0,
            height: 180.0,
        },
        commands: Vec::new(),
        hit_regions: Vec::new(),
        scroll_bounds: Vec::new(),
    })
}

fn commit_for_root(frames: &mut FrameCoordinator<()>, dirty: DirtySet, rows: Signal<Vec<RowData>>) {
    let mut build = frames.begin_build_for_frame(dirty, true);
    let prepared = frames
        .prepare(direct_for_root(&mut build, rows).expect("direct For root assembles"))
        .expect("direct For root prepares");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("direct For root resolves");
    frames.commit(resolved).expect("direct For root commits");
}

fn row_values(frames: &FrameCoordinator<()>) -> Vec<String> {
    frames
        .active()
        .expect("root is active")
        .tree()
        .root()
        .children
        .iter()
        .map(|row| {
            let Some(ContentConcern::Text(text)) =
                row.children.first().and_then(|node| node.content.as_ref())
            else {
                panic!("every row must contain one Text node");
            };
            text.text.clone()
        })
        .collect()
}

fn row_keys(frames: &FrameCoordinator<()>) -> Vec<SemanticKey> {
    let tree = frames.active().expect("root is active").tree();
    tree.keys()
        .iter()
        .filter(|key| tree.path_for_key(key).is_some_and(|path| path.len() == 1))
        .cloned()
        .collect()
}

fn text_values(frames: &FrameCoordinator<()>) -> Vec<String> {
    fn collect(node: &tela_contract::UiNode, values: &mut Vec<String>) {
        if let Some(ContentConcern::Text(text)) = node.content.as_ref() {
            values.push(text.text.clone());
        }
        for child in &node.children {
            collect(child, values);
        }
    }

    let mut values = Vec::new();
    collect(
        frames.active().expect("root is active").tree().root(),
        &mut values,
    );
    values
}

#[test]
fn direct_for_source_survives_empty_state_retries_transactionally_and_preserves_row_keys() {
    let (writer, rows) = signal(Vec::<RowData>::new());
    let mut frames = FrameCoordinator::<()>::new();
    commit_for_root(&mut frames, DirtySet::default(), rows.clone());
    assert!(row_values(&frames).is_empty());

    writer.set_forced(vec![
        RowData {
            id: "first",
            label: "first candidate",
        },
        RowData {
            id: "second",
            label: "second candidate",
        },
    ]);
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    assert_eq!(dirty.len(), 1, "the empty For still owns one source edge");
    assert!(
        dirty.semantic_keys().is_empty(),
        "the edge is lease-owned, not borrowed from a first row or parent node"
    );
    assert!(
        frames
            .prepare_presentation_dirty(dirty.clone())
            .expect("structural dirty is a valid fallback condition")
            .is_none(),
        "a structural source never takes the presentation-only path"
    );
    assert!(
        frames
            .prepare_retained_dirty(dirty.clone())
            .expect("structural dirty is a valid fallback condition")
            .is_none(),
        "a structural source never borrows a retained node root"
    );

    let mut build = frames.begin_build_for_frame(dirty.clone(), true);
    let prepared = frames
        .prepare(direct_for_root(&mut build, rows.clone()).expect("candidate root assembles"))
        .expect("candidate root prepares");
    writer.set_forced(vec![
        RowData {
            id: "first",
            label: "first committed",
        },
        RowData {
            id: "second",
            label: "second committed",
        },
    ]);
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("candidate resolves before the commit freshness barrier");
    assert!(
        frames.commit(resolved).is_err(),
        "late source write rejects candidate"
    );
    assert!(
        row_values(&frames).is_empty(),
        "the rejected candidate did not leak its new structure"
    );
    let retry = frames.runtime().take_dirty();
    assert_eq!(retry.len(), 1, "the structure edge is restored for retry");
    assert!(retry.semantic_keys().is_empty());

    commit_for_root(&mut frames, retry, rows.clone());
    assert_eq!(
        row_values(&frames),
        ["0:first committed", "1:second committed"]
    );
    let before = row_keys(&frames);
    assert_eq!(before.len(), 2);

    writer.set_forced(vec![
        RowData {
            id: "second",
            label: "second committed",
        },
        RowData {
            id: "first",
            label: "first committed",
        },
    ]);
    frames.runtime().begin_frame();
    let reorder_dirty = frames.runtime().take_dirty();
    commit_for_root(&mut frames, reorder_dirty, rows);
    assert_eq!(
        row_values(&frames),
        ["0:second committed", "1:first committed"]
    );
    assert_eq!(
        row_keys(&frames),
        [before[1].clone(), before[0].clone()],
        "For key identity follows the business item through a reorder"
    );
}

#[derive(Clone)]
struct Visibility {
    visible: bool,
    rows: Signal<Vec<RowData>>,
}

fn is_visible(value: &Visibility) -> bool {
    value.visible
}

fn render_visible_rows(
    build: &mut ViewBuild<()>,
    context: ShowContext<Visibility>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column>
            <For each={context.value.rows} key={row_key} row={render_row} />
        </Column>
    })
}

fn render_hidden(
    build: &mut ViewBuild<()>,
    _context: ShowContext<Visibility>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Row><Text value={"hidden"} /></Row>
    })
}

fn show_for_root(
    build: &mut ViewBuild<()>,
    visibility: Signal<Visibility>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column key={"show-for-root"}>
            <Show
                source={visibility}
                test={is_visible}
                then={render_visible_rows}
                fallback={render_hidden}
            />
        </Column>
    })
}

fn commit_show_for_root(
    frames: &mut FrameCoordinator<()>,
    dirty: DirtySet,
    visibility: Signal<Visibility>,
) {
    let mut build = frames.begin_build_for_frame(dirty, true);
    let prepared = frames
        .prepare(show_for_root(&mut build, visibility).expect("Show/For root assembles"))
        .expect("Show/For root prepares");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("Show/For root resolves");
    frames.commit(resolved).expect("Show/For root commits");
}

#[test]
fn show_source_unmounts_nested_for_source_only_after_commit() {
    let (rows_writer, rows) = signal(Vec::<RowData>::new());
    let (visibility_writer, visibility) = signal(Visibility {
        visible: true,
        rows: rows.clone(),
    });
    let mut frames = FrameCoordinator::<()>::new();
    commit_show_for_root(&mut frames, DirtySet::default(), visibility.clone());
    assert!(text_values(&frames).is_empty());

    visibility_writer.set_forced(Visibility {
        visible: false,
        rows: rows.clone(),
    });
    frames.runtime().begin_frame();
    let show_dirty = frames.runtime().take_dirty();
    assert_eq!(
        show_dirty.len(),
        1,
        "Show owns its condition source directly"
    );
    assert!(show_dirty.semantic_keys().is_empty());
    commit_show_for_root(&mut frames, show_dirty, visibility.clone());
    assert_eq!(text_values(&frames), ["hidden"]);

    rows_writer.set_forced(vec![RowData {
        id: "after-unmount",
        label: "must not wake hidden branch",
    }]);
    assert!(
        frames.runtime().take_dirty().is_empty(),
        "For source subscription is removed only once the hiding candidate commits"
    );

    visibility_writer.set_forced(Visibility {
        visible: true,
        rows: rows.clone(),
    });
    frames.runtime().begin_frame();
    let remount_dirty = frames.runtime().take_dirty();
    commit_show_for_root(&mut frames, remount_dirty, visibility);
    assert_eq!(text_values(&frames), ["0:must not wake hidden branch"]);
}

#[test]
fn rejected_show_unmount_keeps_the_active_nested_for_source_subscription() {
    let (rows_writer, rows) = signal(Vec::<RowData>::new());
    let (visibility_writer, visibility) = signal(Visibility {
        visible: true,
        rows: rows.clone(),
    });
    let mut frames = FrameCoordinator::<()>::new();
    commit_show_for_root(&mut frames, DirtySet::default(), visibility.clone());

    visibility_writer.set_forced(Visibility {
        visible: false,
        rows: rows.clone(),
    });
    frames.runtime().begin_frame();
    let dirty = frames.runtime().take_dirty();
    let mut build = frames.begin_build_for_frame(dirty, true);
    let prepared = frames
        .prepare(show_for_root(&mut build, visibility.clone()).expect("hiding candidate assembles"))
        .expect("hiding candidate prepares");

    visibility_writer.set_forced(Visibility {
        visible: true,
        rows: rows.clone(),
    });
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("stale hiding candidate still resolves");
    assert!(
        frames.commit(resolved).is_err(),
        "a condition source change rejects the hiding candidate"
    );
    assert!(
        text_values(&frames).is_empty(),
        "the active visible branch is not replaced by a rejected hiding candidate"
    );

    let retry = frames.runtime().take_dirty();
    assert_eq!(
        retry.len(),
        1,
        "the stale Show source is restored for retry"
    );

    rows_writer.set_forced(vec![RowData {
        id: "still-mounted",
        label: "active nested source remains subscribed",
    }]);
    let row_dirty = frames.runtime().take_dirty();
    assert_eq!(
        row_dirty.len(),
        1,
        "the failed unmount did not tear down the active nested For subscription"
    );
    assert!(row_dirty.semantic_keys().is_empty());

    commit_show_for_root(&mut frames, row_dirty, visibility);
    assert_eq!(
        text_values(&frames),
        ["0:active nested source remains subscribed"]
    );
}

#[derive(Clone)]
enum Screen {
    Loading,
    Ready(Signal<Vec<RowData>>),
    Error,
}

fn screen_branch(screen: &Screen) -> String {
    match screen {
        Screen::Loading => "loading".to_owned(),
        Screen::Ready(_) => "ready".to_owned(),
        Screen::Error => "error".to_owned(),
    }
}

fn render_screen(
    build: &mut ViewBuild<()>,
    context: SwitchContext<Screen>,
) -> ViewResult<ViewOutput<()>> {
    match context.value {
        Screen::Loading => ui!(build {
            <Row><Text value={"loading"} /></Row>
        }),
        Screen::Ready(rows) => ui!(build {
            <Column>
                <For each={rows} key={row_key} row={render_row} />
            </Column>
        }),
        Screen::Error => ui!(build {
            <Row><Text value={"error"} /></Row>
        }),
    }
}

fn switch_for_root(
    build: &mut ViewBuild<()>,
    screen: Signal<Screen>,
) -> ViewResult<ViewOutput<()>> {
    ui!(build {
        <Column key={"switch-for-root"}>
            <Switch source={screen} branch={screen_branch} render={render_screen} />
        </Column>
    })
}

fn commit_switch_for_root(
    frames: &mut FrameCoordinator<()>,
    dirty: DirtySet,
    screen: Signal<Screen>,
) {
    let mut build = frames.begin_build_for_frame(dirty, true);
    let prepared = frames
        .prepare(switch_for_root(&mut build, screen).expect("Switch/For root assembles"))
        .expect("Switch/For root prepares");
    let resolved = prepared
        .resolve(|_| Ok::<_, ()>(empty_frame()))
        .expect("Switch/For root resolves");
    frames.commit(resolved).expect("Switch/For root commits");
}

#[test]
fn switch_source_selects_named_branches_and_unmounts_nested_sources_after_commit() {
    let (rows_writer, rows) = signal(Vec::<RowData>::new());
    let (screen_writer, screen) = signal(Screen::Ready(rows.clone()));
    let mut frames = FrameCoordinator::<()>::new();
    commit_switch_for_root(&mut frames, DirtySet::default(), screen.clone());
    assert!(text_values(&frames).is_empty());

    screen_writer.set_forced(Screen::Loading);
    frames.runtime().begin_frame();
    let switch_dirty = frames.runtime().take_dirty();
    assert_eq!(
        switch_dirty.len(),
        1,
        "Switch owns its choice source instead of borrowing a branch node"
    );
    assert!(switch_dirty.semantic_keys().is_empty());
    commit_switch_for_root(&mut frames, switch_dirty, screen.clone());
    assert_eq!(text_values(&frames), ["loading"]);

    rows_writer.set_forced(vec![RowData {
        id: "after-ready-unmount",
        label: "must not wake loading branch",
    }]);
    assert!(
        frames.runtime().take_dirty().is_empty(),
        "the nested For subscription is removed only once the new Switch branch commits"
    );

    screen_writer.set_forced(Screen::Error);
    frames.runtime().begin_frame();
    let error_dirty = frames.runtime().take_dirty();
    commit_switch_for_root(&mut frames, error_dirty, screen.clone());
    assert_eq!(text_values(&frames), ["error"]);

    screen_writer.set_forced(Screen::Ready(rows));
    frames.runtime().begin_frame();
    let remount_dirty = frames.runtime().take_dirty();
    commit_switch_for_root(&mut frames, remount_dirty, screen);
    assert_eq!(
        text_values(&frames),
        ["0:must not wake loading branch"],
        "remounting the ready branch observes its current explicit source snapshot"
    );
}
