//! 应用运行时：组合领域、View、tela-core 视图状态与布局缓存。

use std::collections::{BTreeSet, HashMap};

use tela_contract::{
    InputEvent, NodeId, NodeKind, PointerEvent, ScrollState, SemanticKey, TextMeasureRequest,
    TextMeasurer, TextMetrics, UiAction, UiFrame, UiNode, Viewport,
};
use tela_core::{LayoutCache, UiTree, ViewStateStore, handle_input};
use tela_ui::{DraftInputEvent, IntentTarget, LocalStateRuntime, UiIntent, intent_from_action};

use super::reactive::ComponentRuntime;
use super::{Intent, apply_intent, intent_from_bind_id};
use crate::domain::{FileManagerModel, FileManagerSession};
use crate::presentation::operation::OPERATION_MODAL_KEY;
use crate::presentation::{
    component::Component,
    shell::{AppShell, AppShellProps},
};

/// 初次加载默认逻辑尺寸；浏览器启动后由宿主覆盖为实际 CSS 视口。
pub const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 1280.0,
    height: 800.0,
};

struct DemoTextMeasurer;

impl TextMeasurer for DemoTextMeasurer {
    fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics {
        let line_count = request.text.lines().count().max(1) as u32;
        let width = request
            .text
            .lines()
            .map(|line| {
                line.chars()
                    .map(|c| {
                        if c.is_ascii() {
                            request.font_size * 0.56
                        } else {
                            request.font_size
                        }
                    })
                    .sum::<f32>()
            })
            .fold(0.0, f32::max);
        TextMetrics {
            width: request.max_width.map_or(width, |max| width.min(max)),
            height: line_count as f32 * request.line_height,
            line_count,
        }
    }
}

/// 跨帧会话。业务数据、临时 view state 与 renderer 缓存各自隔离。
pub struct App {
    pub(crate) model: FileManagerModel,
    pub(crate) session: FileManagerSession,
    viewport: Viewport,
    raster_dpi: f32,
    frame: Option<UiFrame>,
    tree: Option<UiTree>,
    layout_cache: LayoutCache,
    view_state: ViewStateStore,
    search_key: Option<SemanticKey>,
    nav_scroll_key: Option<SemanticKey>,
    detail_scroll_key: Option<SemanticKey>,
    clickable_keys: BTreeSet<SemanticKey>,
    hovered_toolbar_target: Option<String>,
    nav_scroll: ScrollState,
    detail_scroll: ScrollState,
    frame_trace: Vec<u8>,
    cpu_rendered: bool,
    cpu_bitmap: Vec<u8>,
    input_upload: Vec<u8>,
    revision: tela_widgets::Signal<u64>,
    component_runtime: ComponentRuntime,
    local_state: LocalStateRuntime,
}

impl App {
    pub fn new() -> Self {
        let revision = tela_widgets::Signal::new(0);
        let mut component_runtime = ComponentRuntime::new();
        component_runtime.watch("app.shell", &revision);
        Self {
            model: FileManagerModel::sample(),
            session: FileManagerSession::default(),
            viewport: DEFAULT_VIEWPORT,
            raster_dpi: 1.0,
            frame: None,
            tree: None,
            layout_cache: LayoutCache::new(),
            view_state: ViewStateStore::new(),
            search_key: None,
            nav_scroll_key: None,
            detail_scroll_key: None,
            clickable_keys: BTreeSet::new(),
            hovered_toolbar_target: None,
            nav_scroll: ScrollState::default(),
            detail_scroll: ScrollState::default(),
            frame_trace: Vec::new(),
            cpu_rendered: false,
            cpu_bitmap: Vec::new(),
            input_upload: Vec::new(),
            revision,
            component_runtime,
            local_state: LocalStateRuntime::new(),
        }
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) -> bool {
        let viewport = Viewport {
            width: width.max(320.0),
            height: height.max(240.0),
        };
        if self.viewport == viewport {
            return false;
        }
        self.viewport = viewport;
        self.invalidate_frame();
        true
    }

    pub fn set_raster_dpi(&mut self, dpi: f32) {
        let dpi = dpi.clamp(1.0, 3.0);
        if (dpi - self.raster_dpi).abs() > f32::EPSILON {
            self.raster_dpi = dpi;
            self.cpu_rendered = false;
        }
    }

    pub fn ensure_frame(&mut self) -> bool {
        if !self.component_runtime.take_dirty().is_empty() {
            self.invalidate_frame();
        }
        if self.frame.is_some() {
            return false;
        }
        let modal_key = SemanticKey(OPERATION_MODAL_KEY.to_owned());
        if self.session.operation.is_some() && !self.view_state.modal_stack().contains(&modal_key) {
            self.view_state.push_modal(modal_key.clone());
        }
        if self.session.operation.is_none()
            && self.view_state.modal_stack().last() == Some(&modal_key)
        {
            self.view_state.pop_modal();
        }
        self.local_state.begin_render();
        let search_input = self
            .local_state
            .sync_draft_input(&self.session.query, "file.search");
        let operation_input = self.session.operation.as_ref().and_then(|operation| {
            if matches!(
                operation.kind,
                crate::domain::OperationKind::MoveToDesign | crate::domain::OperationKind::Trash
            ) {
                None
            } else {
                Some(
                    self.local_state
                        .sync_draft_input(&operation.value, "operation.value"),
                )
            }
        });
        let props = AppShellProps {
            model: &self.model,
            session: &self.session,
            viewport: self.viewport,
            search_focused: self.search_focused(),
            hovered_target: self.hovered_toolbar_target.clone(),
            search_input,
            operation_input,
        };
        let tree = UiTree::new(AppShell.render(&props)).expect("文件管理器场景必须合法");
        self.local_state.finish_render();
        let mut scroll_inputs = HashMap::new();
        if let Some(key) = &self.nav_scroll_key {
            scroll_inputs.insert(key.clone(), self.nav_scroll);
        }
        if let Some(key) = &self.detail_scroll_key {
            scroll_inputs.insert(key.clone(), self.detail_scroll);
        }
        let frame = tree
            .resolve_dirty(
                self.viewport,
                &DemoTextMeasurer,
                &scroll_inputs,
                &mut self.layout_cache,
            )
            .expect("文件管理器场景必须可布局");
        self.frame_trace = crate::frame_trace::to_json(&frame).into_bytes();
        let controls = discover_controls(&tree);
        self.search_key = controls.search;
        self.nav_scroll_key = controls.scrolls.first().cloned();
        self.detail_scroll_key = controls.scrolls.get(1).cloned();
        self.clickable_keys = controls.clickable;
        self.tree = Some(tree);
        self.frame = Some(frame);
        self.cpu_rendered = false;
        true
    }

    pub fn frame(&self) -> &UiFrame {
        self.frame.as_ref().expect("共享逻辑帧必须已构建")
    }
    #[cfg(feature = "webgpu")]
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }
    pub fn frame_trace(&self) -> &[u8] {
        &self.frame_trace
    }
    pub fn cpu_bitmap(&self) -> &[u8] {
        &self.cpu_bitmap
    }
    pub fn raster_size(&self) -> (u32, u32) {
        (
            (self.viewport.width * self.raster_dpi).round() as u32,
            (self.viewport.height * self.raster_dpi).round() as u32,
        )
    }
    pub fn input_focused(&self) -> bool {
        if self.session.operation.is_some() {
            self.operation_accepts_input()
        } else {
            self.search_focused()
        }
    }
    pub fn input_value(&self) -> String {
        if self.operation_accepts_input() {
            return self
                .local_state
                .snapshot(&IntentTarget::new("operation.value"))
                .map(|snapshot| snapshot.value().to_owned())
                .unwrap_or_else(|| {
                    self.session
                        .operation
                        .as_ref()
                        .map(|operation| operation.value.clone())
                        .unwrap_or_default()
                });
        }
        self.local_state
            .snapshot(&IntentTarget::new("file.search"))
            .map(|snapshot| snapshot.value().to_owned())
            .unwrap_or_else(|| self.session.query.clone())
    }
    pub fn pointer_cursor(&self) -> u32 {
        if self.search_focused() {
            1
        } else if self
            .view_state
            .hover_key()
            .is_some_and(|key| self.clickable_keys.contains(key))
        {
            2
        } else {
            0
        }
    }

    pub fn render_cpu_if_needed(&mut self) -> bool {
        self.ensure_frame();
        if self.cpu_rendered {
            return false;
        }
        let mut config = tela_render_raster::RasterConfig::default_with(
            tela_contract::Color::rgba(0.96, 0.97, 0.99, 1.0),
        );
        config.dpi_scale = self.raster_dpi;
        self.cpu_bitmap = tela_render_raster::render_frame(self.frame(), &config).pixels;
        self.cpu_rendered = true;
        true
    }

    pub fn handle_pointer(&mut self, event: PointerEvent) -> u32 {
        self.ensure_frame();
        let frame = self.frame().clone();
        let tree = self.tree.as_ref().expect("tree");
        let actions = handle_input(
            tree,
            &frame,
            &mut self.view_state,
            &InputEvent::Pointer(event),
        );
        let mut changed = false;
        for action in &actions {
            match action {
                UiAction::Click { node_id } => changed |= self.handle_click(*node_id),
                UiAction::Scroll { node_id, delta } => {
                    changed |= self.handle_scroll(*node_id, delta.y)
                }
                UiAction::Hover { node_id, .. } => {
                    self.hovered_toolbar_target = self.toolbar_target_at(*node_id);
                    changed = true;
                }
                UiAction::RequestFocus { .. } | UiAction::FocusChanged { .. } => changed = true,
                _ => {}
            }
        }
        if changed {
            self.mark_view_dirty();
        }
        actions.len() as u32
    }

    pub fn set_input_value(&mut self, value: String) -> u32 {
        self.ensure_frame();
        if self.session.operation.is_some() && !self.operation_accepts_input() {
            return 0;
        }
        if self.session.operation.is_none() && !self.search_focused() {
            return 0;
        }
        self.dispatch_draft_input(DraftInputEvent::Input(value))
    }

    pub fn composition_start(&mut self) -> u32 {
        self.ensure_frame();
        self.dispatch_draft_input(DraftInputEvent::CompositionStart)
    }

    pub fn composition_end(&mut self) -> u32 {
        self.ensure_frame();
        self.dispatch_draft_input(DraftInputEvent::CompositionEnd)
    }

    pub fn input_enter(&mut self) -> u32 {
        self.ensure_frame();
        self.dispatch_draft_input(DraftInputEvent::Enter)
    }

    pub fn input_cancel(&mut self) -> u32 {
        self.ensure_frame();
        self.dispatch_draft_input(DraftInputEvent::Cancel)
    }

    pub fn input_blur(&mut self) -> u32 {
        self.ensure_frame();
        self.dispatch_draft_input(DraftInputEvent::Blur)
    }

    #[cfg(test)]
    fn dispatch_bind_id(&mut self, bind_id: &str) -> bool {
        let Some(intent) = intent_from_bind_id(bind_id) else {
            return false;
        };
        self.dispatch_controller_intent(intent);
        self.mark_view_dirty();
        true
    }

    pub fn begin_input_upload(&mut self, bytes: usize) -> *mut u8 {
        self.input_upload.resize(bytes, 0);
        self.input_upload.as_mut_ptr()
    }
    pub fn input_value_ptr(&mut self) -> *const u8 {
        self.input_upload = self.input_value().into_bytes();
        self.input_upload.as_ptr()
    }
    pub fn input_value_len(&mut self) -> u32 {
        self.input_value().len() as u32
    }
    pub fn finish_input_upload(&mut self, bytes: usize) -> u32 {
        if bytes != self.input_upload.len() {
            self.input_upload.clear();
            return 0;
        }
        let Ok(value) = String::from_utf8(std::mem::take(&mut self.input_upload)) else {
            return 0;
        };
        self.set_input_value(value)
    }

    fn invalidate_frame(&mut self) {
        self.frame = None;
        self.tree = None;
        self.frame_trace.clear();
        self.cpu_rendered = false;
    }
    fn mark_view_dirty(&mut self) {
        self.revision.update(|value| *value = value.wrapping_add(1));
    }
    fn dispatch_draft_input(&mut self, event: DraftInputEvent) -> u32 {
        let target = if self.operation_accepts_input() {
            IntentTarget::new("operation.value")
        } else if self.session.operation.is_none() && self.search_focused() {
            IntentTarget::new("file.search")
        } else {
            return 0;
        };
        let Some(outcome) = self.local_state.dispatch(&target, event) else {
            return 0;
        };
        let mut changed = outcome.changed;
        if let Some(UiIntent::Commit {
            target,
            value: tela_contract::Value::String(value),
        }) = outcome.intent
        {
            let intent = match target.as_str() {
                "operation.value" => Intent::SetOperationValue(value),
                "file.search" => {
                    self.detail_scroll = ScrollState::default();
                    Intent::SetQuery(value)
                }
                _ => return u32::from(changed),
            };
            self.apply_controller_intent(intent);
            changed = true;
        }
        if changed {
            self.mark_view_dirty();
        }
        u32::from(changed)
    }
    fn apply_controller_intent(&mut self, intent: Intent) {
        apply_intent(&mut self.model, &mut self.session, intent);
        if self.session.operation.is_none() {
            self.local_state
                .release_target(&IntentTarget::new("operation.value"));
        }
    }
    fn dispatch_controller_intent(&mut self, intent: Intent) {
        if matches!(intent, Intent::ConfirmOperation) {
            self.dispatch_draft_input(DraftInputEvent::Blur);
        }
        self.apply_controller_intent(intent);
    }
    fn search_focused(&self) -> bool {
        self.view_state
            .current_focus_key()
            .is_some_and(|key| self.search_key.as_ref() == Some(key))
    }
    fn operation_accepts_input(&self) -> bool {
        self.session.operation.as_ref().is_some_and(|operation| {
            !matches!(
                operation.kind,
                crate::domain::OperationKind::MoveToDesign | crate::domain::OperationKind::Trash
            )
        })
    }

    fn toolbar_target_at(&self, node_id: NodeId) -> Option<String> {
        self.tree
            .as_ref()
            .and_then(|tree| node_at(tree.root(), node_id.0 as usize, &mut 0))
            .and_then(|node| node.interact.as_ref())
            .and_then(|interact| interact.bind_id.as_ref())
            .and_then(|bind_id| bind_id.0.strip_prefix("ui.invoke:"))
            .map(str::to_owned)
    }

    fn handle_click(&mut self, node_id: NodeId) -> bool {
        let Some(tree) = self.tree.as_ref() else {
            return false;
        };
        let Some(node) = node_at(tree.root(), node_id.0 as usize, &mut 0) else {
            return false;
        };
        let Some(bind_id) = node
            .interact
            .as_ref()
            .and_then(|interact| interact.bind_id.as_ref())
        else {
            return false;
        };
        let action = UiAction::Click { node_id };
        let intent = intent_from_action(&action, Some(bind_id))
            .and_then(|intent| match intent {
                tela_ui::UiIntent::Invoke { target } => intent_from_bind_id(target.as_str()),
                tela_ui::UiIntent::Preview { .. } | tela_ui::UiIntent::Commit { .. } => None,
            })
            .or_else(|| intent_from_bind_id(&bind_id.0));
        let Some(intent) = intent else {
            return false;
        };
        if self.session.operation.is_some() && !bind_id.0.starts_with("operation.") {
            return false;
        }
        self.dispatch_controller_intent(intent);
        self.mark_view_dirty();
        true
    }

    fn handle_scroll(&mut self, node_id: NodeId, delta_y: f32) -> bool {
        let Some(tree) = self.tree.as_ref() else {
            return false;
        };
        let Some(key) = node_key(tree, node_id).cloned() else {
            return false;
        };
        let detail_h = (self.viewport.height - 48.0 - 40.0 - 36.0 - 28.0 - 92.0).max(80.0);
        let (state, max) = if self.nav_scroll_key.as_ref() == Some(&key) {
            (&mut self.nav_scroll, 360.0)
        } else if self.detail_scroll_key.as_ref() == Some(&key) {
            (&mut self.detail_scroll, (96.0 * 30.0 - detail_h).max(0.0))
        } else {
            return false;
        };
        let next = (state.offset_y + delta_y).clamp(0.0, max);
        if (next - state.offset_y).abs() < f32::EPSILON {
            return false;
        }
        state.offset_y = next;
        true
    }
}

struct Controls {
    search: Option<SemanticKey>,
    scrolls: Vec<SemanticKey>,
    clickable: BTreeSet<SemanticKey>,
}
fn discover_controls(tree: &UiTree) -> Controls {
    fn visit(node: &UiNode, keys: &[SemanticKey], i: &mut usize, out: &mut Controls) {
        let key = keys.get(*i).cloned();
        *i += 1;
        if let Some(key) = key {
            if node
                .interact
                .as_ref()
                .is_some_and(|interact| interact.text_input)
            {
                out.search = Some(key.clone());
            }
            if node
                .interact
                .as_ref()
                .is_some_and(|interact| interact.clickable)
            {
                out.clickable.insert(key.clone());
            }
            if matches!(
                node.kind,
                NodeKind::ScrollView | NodeKind::VirtualListView(_)
            ) {
                out.scrolls.push(key);
            }
        }
        for child in &node.children {
            visit(child, keys, i, out);
        }
    }
    let mut out = Controls {
        search: None,
        scrolls: Vec::new(),
        clickable: BTreeSet::new(),
    };
    visit(tree.root(), tree.keys(), &mut 0, &mut out);
    out
}
fn node_at<'a>(node: &'a UiNode, target: usize, i: &mut usize) -> Option<&'a UiNode> {
    if *i == target {
        return Some(node);
    }
    *i += 1;
    for child in &node.children {
        if let Some(found) = node_at(child, target, i) {
            return Some(found);
        }
    }
    None
}
fn node_key(tree: &UiTree, id: NodeId) -> Option<&SemanticKey> {
    tree.node_ids()
        .iter()
        .position(|candidate| *candidate == id)
        .and_then(|index| tree.keys().get(index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FileCommand;

    fn click_bound(app: &mut App, bind_id: &str) {
        app.ensure_frame();
        let tree = app.tree.as_ref().expect("tree");
        let node_id = tree
            .node_ids()
            .iter()
            .copied()
            .find(|id| {
                node_at(tree.root(), id.0 as usize, &mut 0)
                    .and_then(|node| node.interact.as_ref())
                    .and_then(|interact| interact.bind_id.as_ref())
                    .is_some_and(|bound| {
                        bound.0 == bind_id || bound.0 == format!("ui.invoke:{bind_id}")
                    })
            })
            .expect("交互绑定应存在");
        let hit = app
            .frame()
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("交互绑定应有命中区");
        app.handle_pointer(PointerEvent::Down {
            position: tela_contract::Point {
                x: hit.rect.x + hit.rect.w.min(320.0) / 2.0,
                y: hit.rect.y + hit.rect.h / 2.0,
            },
        });
    }

    #[test]
    fn file_manager_shell_is_full_viewport_and_contains_client_regions() {
        let mut app = App::new();
        app.set_viewport(1440.0, 900.0);
        assert!(app.ensure_frame());
        assert_eq!(
            app.frame().viewport,
            Viewport {
                width: 1440.0,
                height: 900.0
            }
        );
        let labels: Vec<String> = app
            .frame()
            .commands
            .iter()
            .filter_map(|command| match &command.payload {
                tela_contract::DrawPayload::Text { text } => Some(text.text.clone()),
                _ => None,
            })
            .collect();
        for label in ["TELA 文件", "新建", "工作区", "README.md"] {
            assert!(labels.contains(&label.to_owned()), "缺少 {label}");
        }
    }

    #[test]
    fn selecting_a_text_file_switches_to_read_only_preview() {
        let mut app = App::new();
        app.session.select(5);
        app.ensure_frame();
        let trace = std::str::from_utf8(app.frame_trace()).unwrap();
        assert!(trace.contains("Tela 工作区"));
        assert!(trace.contains("只读说明"));
    }

    #[test]
    fn controller_executes_memory_commands_without_manual_tela_keys() {
        let mut app = App::new();
        app.session.select(5);
        app.model.apply(&mut app.session, FileCommand::CopySelected);
        let copied = *app.session.selected.iter().next().unwrap();
        assert_ne!(copied, 5);
        app.model.apply(&mut app.session, FileCommand::Undo);
        assert!(app.model.entry(copied).is_none());
    }

    #[test]
    fn component_bindings_route_selection_directory_navigation_and_commands() {
        let mut app = App::new();
        assert!(app.dispatch_bind_id("entry.select.5"));
        assert_eq!(app.session.selected, BTreeSet::from([5]));
        assert!(app.dispatch_bind_id("command.toggle-view"));
        assert_eq!(app.session.view, crate::domain::DirectoryView::Grid);
        assert!(app.dispatch_bind_id("filter.favorites"));
        assert_eq!(app.session.filter, crate::domain::EntryFilter::Favorites);
        assert!(app.dispatch_bind_id("folder.open.2"));
        assert_eq!(app.session.current_dir, 2);
        assert_eq!(app.session.filter, crate::domain::EntryFilter::All);
        assert!(app.dispatch_bind_id("command.new-folder"));
        assert!(app.dispatch_bind_id("operation.confirm"));
        assert!(
            app.model
                .entries_in_filtered(2, "", app.session.filter, app.session.sort)
                .iter()
                .any(|entry| entry.name.starts_with("新建文件夹"))
        );
    }

    #[test]
    fn viewport_breakpoints_keep_a_fixed_client_root() {
        let mut app = App::new();
        for (width, height) in [(1440.0, 900.0), (1199.0, 800.0), (899.0, 720.0)] {
            app.set_viewport(width, height);
            assert!(app.ensure_frame());
            assert_eq!(app.frame().viewport, Viewport { width, height });
            assert!(
                app.frame()
                    .commands
                    .iter()
                    .any(|command| matches!(&command.payload,
                tela_contract::DrawPayload::Text { text } if text.text == "TELA 文件"))
            );
            app.invalidate_frame();
        }
    }

    #[test]
    fn operation_modal_requires_confirm_and_writes_its_controlled_draft() {
        let mut app = App::new();
        assert!(app.dispatch_bind_id("command.new-folder"));
        assert_eq!(
            app.session.operation.as_ref().map(|draft| &draft.value),
            Some(&"新建文件夹".to_owned())
        );
        assert_eq!(app.set_input_value("验收目录".to_owned()), 1);
        assert_eq!(app.input_enter(), 1);
        assert!(
            app.model
                .entries_in_filtered(1, "", app.session.filter, app.session.sort)
                .iter()
                .all(|entry| entry.name != "验收目录")
        );
        assert!(app.dispatch_bind_id("operation.confirm"));
        assert!(app.session.operation.is_none());
        assert!(
            app.model
                .entries_in_filtered(1, "", app.session.filter, app.session.sort)
                .iter()
                .any(|entry| entry.name == "验收目录")
        );
        assert!(app.dispatch_bind_id("command.rename"));
        assert!(app.dispatch_bind_id("operation.cancel"));
        assert!(app.session.operation.is_none());
        assert_eq!(app.session.notice, "已取消操作");
    }

    #[test]
    fn operation_draft_commits_at_boundaries_and_does_not_survive_a_reopen() {
        let mut app = App::new();
        assert!(app.dispatch_bind_id("command.new-folder"));
        assert_eq!(app.set_input_value("仅本地草稿".to_owned()), 1);
        assert_eq!(
            app.session
                .operation
                .as_ref()
                .map(|draft| draft.value.as_str()),
            Some("新建文件夹")
        );
        assert_eq!(app.composition_start(), 1);
        assert_eq!(app.input_enter(), 0, "IME 组合期间不能提交");
        assert_eq!(app.composition_end(), 1);
        assert_eq!(app.input_blur(), 1);
        assert_eq!(
            app.session
                .operation
                .as_ref()
                .map(|draft| draft.value.as_str()),
            Some("仅本地草稿")
        );
        assert!(app.dispatch_bind_id("operation.cancel"));
        assert!(app.dispatch_bind_id("command.add-tag"));
        app.ensure_frame();
        assert_eq!(app.input_value(), "重点");
        assert_eq!(app.set_input_value("临时标签".to_owned()), 1);
        assert_eq!(app.input_cancel(), 1);
        assert_eq!(app.input_value(), "重点");
        assert!(app.dispatch_bind_id("operation.cancel"));
        assert!(app.dispatch_bind_id("command.add-tag"));
        app.ensure_frame();
        assert_eq!(app.input_value(), "重点");

        app.session.select(5);
        assert!(app.dispatch_bind_id("command.rename"));
        assert_eq!(app.set_input_value("README-已重命名.md".to_owned()), 1);
        assert!(app.dispatch_bind_id("operation.confirm"));
        assert_eq!(
            app.model.entry(5).map(|entry| entry.name.as_str()),
            Some("README-已重命名.md")
        );
    }

    #[test]
    fn narrow_navigation_overlays_instead_of_shrinking_the_detail_pane() {
        fn readme_x(app: &App) -> f32 {
            app.frame()
                .commands
                .iter()
                .find_map(|command| match &command.payload {
                    tela_contract::DrawPayload::Text { text } if text.text == "README.md" => {
                        Some(command.geometry.x)
                    }
                    _ => None,
                })
                .expect("README 应显示")
        }
        let mut app = App::new();
        app.set_viewport(1199.0, 800.0);
        app.ensure_frame();
        let before = readme_x(&app);
        assert!(app.dispatch_bind_id("navigation.toggle"));
        app.ensure_frame();
        assert_eq!(readme_x(&app), before);
        let trace = std::str::from_utf8(app.frame_trace()).unwrap();
        assert!(trace.contains("文件夹"), "窄屏抽屉应显示目录树");
    }

    #[test]
    fn canvas_hit_testing_routes_bound_actions_through_core() {
        let mut app = App::new();
        app.ensure_frame();
        assert!(app.frame().hit_regions.iter().all(|region| {
            region.rect.x.is_finite()
                && region.rect.y.is_finite()
                && region.rect.w.is_finite()
                && region.rect.h.is_finite()
        }));
        click_bound(&mut app, "entry.select.5");
        assert_eq!(app.session.selected, BTreeSet::from([5]));
        click_bound(&mut app, "folder.open.1");
        assert_eq!(app.session.current_dir, 1);
        click_bound(&mut app, "folder.open.2");
        assert_eq!(app.session.current_dir, 2);
        click_bound(&mut app, "entry.select.8");
        click_bound(&mut app, "command.rename");
        assert!(app.session.operation.is_some());
        click_bound(&mut app, "operation.confirm");
        assert!(app.session.operation.is_none());
        assert_eq!(app.model.entry(5).expect("README 存在").name, "README.md");
    }

    #[test]
    fn toolbar_hover_is_projected_from_core_view_state_without_a_component_key() {
        let mut app = App::new();
        app.ensure_frame();
        let tree = app.tree.as_ref().expect("tree");
        let node_id = tree
            .node_ids()
            .iter()
            .copied()
            .find(|id| {
                node_at(tree.root(), id.0 as usize, &mut 0)
                    .and_then(|node| node.interact.as_ref())
                    .and_then(|interact| interact.bind_id.as_ref())
                    .is_some_and(|bind| bind.0 == "ui.invoke:command.new-folder")
            })
            .expect("Toolbar 新建项应存在");
        let hit = app
            .frame()
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("Toolbar 新建项应可命中");
        app.handle_pointer(PointerEvent::Move {
            position: tela_contract::Point {
                x: hit.rect.x + 1.0,
                y: hit.rect.y + 1.0,
            },
        });
        assert_eq!(
            app.hovered_toolbar_target.as_deref(),
            Some("command.new-folder")
        );
        assert!(app.ensure_frame());
    }
}
