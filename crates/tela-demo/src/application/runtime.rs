//! 应用运行时：组合领域、View、tela-core 视图状态与布局缓存。

mod input;

use std::collections::{BTreeSet, HashMap};

use ab_glyph::{Font, FontArc, ScaleFont};
use tela_contract::{
    Color, FocusAppearance, InputEvent, NodeId, NodeKind, PointerEvent, RawKeyboardEvent,
    ScrollState, SemanticKey, ShortcutId, TextMeasureRequest, TextMeasurer, TextMetrics, UiAction,
    UiFrame, UiNode, Viewport,
};
use tela_core::{
    IdentityAllocator, LayoutCache, UiTree, ViewStateStore, ensure_modal_focus, handle_input,
    restore_focus, save_focus,
};
use tela_ui::{LocalStateRuntime, intent_from_action};

use super::keymap::{KeymapError, KeymapSnapshot, raw_key_from_codes};
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

const FOCUS_APPEARANCE: FocusAppearance = FocusAppearance {
    color: Color::rgba(0.15, 0.39, 0.92, 1.0),
    width: 2.0,
    inset: 2.0,
};

struct DemoTextMeasurer;

impl TextMeasurer for DemoTextMeasurer {
    fn measure(&self, request: &TextMeasureRequest<'_>) -> TextMetrics {
        let font = demo_font(request.font);
        let scaled = font.as_scaled(em_pixel_height(font, request.font_size));
        let line_count = request.text.lines().count().max(1) as u32;
        let width = request
            .text
            .lines()
            .map(|line| {
                line.chars()
                    .map(|character| scaled.h_advance(scaled.glyph_id(character)))
                    .sum::<f32>()
            })
            .fold(0.0, f32::max);
        TextMetrics {
            width: request.max_width.map_or(width, |max| width.min(max)),
            height: line_count as f32 * request.line_height,
            line_count,
            first_baseline: scaled.ascent(),
        }
    }
}

fn demo_font(font: &tela_contract::FontRef) -> &'static FontArc {
    use std::sync::OnceLock;
    static UI_FONT: OnceLock<FontArc> = OnceLock::new();
    static ICON_FONT: OnceLock<FontArc> = OnceLock::new();
    if font.0 == tela_fonts::ICON_FONT_NAME {
        ICON_FONT.get_or_init(|| {
            FontArc::try_from_slice(tela_fonts::ICON_FONT_BYTES).expect("图标字体必须可解析")
        })
    } else {
        UI_FONT.get_or_init(|| {
            FontArc::try_from_slice(tela_fonts::UI_FONT_BYTES).expect("正文字体必须可解析")
        })
    }
}

fn em_pixel_height(font: &FontArc, font_size: f32) -> f32 {
    font_size * font.height_unscaled() / font.units_per_em().unwrap_or(1000.0)
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
    identity_allocator: IdentityAllocator,
    view_state: ViewStateStore,
    nav_scroll_key: Option<SemanticKey>,
    detail_scroll_key: Option<SemanticKey>,
    clickable_keys: BTreeSet<SemanticKey>,
    hovered_toolbar_target: Option<String>,
    keymap: KeymapSnapshot,
    nav_scroll: ScrollState,
    detail_scroll: ScrollState,
    frame_trace: Vec<u8>,
    cpu_rendered: bool,
    cpu_bitmap: Vec<u8>,
    input_upload: Vec<u8>,
    keymap_upload: Vec<u8>,
    /// 由 `runtime::input` 管理的隐藏 DOM 编辑器目标，不是 tela key 或业务状态。
    dom_input_target: Option<tela_ui::IntentTarget>,
    /// 弹窗关闭后的显式焦点恢复延迟到新树建好后执行，避免把旧帧 node id 带回页面。
    restore_focus_pending: bool,
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
            identity_allocator: IdentityAllocator::new(),
            view_state: ViewStateStore::new(),
            nav_scroll_key: None,
            detail_scroll_key: None,
            clickable_keys: BTreeSet::new(),
            hovered_toolbar_target: None,
            keymap: KeymapSnapshot::file_manager_default(),
            nav_scroll: ScrollState::default(),
            detail_scroll: ScrollState::default(),
            frame_trace: Vec::new(),
            cpu_rendered: false,
            cpu_bitmap: Vec::new(),
            input_upload: Vec::new(),
            keymap_upload: Vec::new(),
            dom_input_target: None,
            restore_focus_pending: false,
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
            save_focus(&mut self.view_state);
            self.view_state.push_modal(modal_key.clone());
        }
        if self.session.operation.is_none()
            && self.view_state.modal_stack().last() == Some(&modal_key)
        {
            self.view_state.pop_modal();
            self.restore_focus_pending = true;
        }
        let (search_input, operation_input) = self.begin_input_render();
        let mut props = AppShellProps {
            model: &self.model,
            session: &self.session,
            viewport: self.viewport,
            search_focused: false,
            operation_focused: false,
            hovered_target: self.hovered_toolbar_target.clone(),
            search_input,
            operation_input,
        };
        let mut tree =
            UiTree::new_with_allocator(AppShell.render(&props), &mut self.identity_allocator)
                .expect("文件管理器场景必须合法");
        let mut scroll_inputs = HashMap::new();
        if let Some(key) = &self.nav_scroll_key {
            scroll_inputs.insert(key.clone(), self.nav_scroll);
        }
        if let Some(key) = &self.detail_scroll_key {
            scroll_inputs.insert(key.clone(), self.detail_scroll);
        }
        let focusable_nodes = tree.focusable_nodes();
        self.view_state.reconcile_focus(&focusable_nodes);
        self.view_state.reconcile_hover(tree.keys());
        if self.restore_focus_pending {
            restore_focus(&tree, &mut self.view_state);
            self.restore_focus_pending = false;
        }
        ensure_modal_focus(&tree, &mut self.view_state);
        let mut controls = discover_controls(&tree);
        let hovered_target = self.toolbar_target_for_hover_key(&tree);
        if self.hovered_toolbar_target != hovered_target {
            self.hovered_toolbar_target = hovered_target;
            props.hovered_target = self.hovered_toolbar_target.clone();
            tree =
                UiTree::new_with_allocator(AppShell.render(&props), &mut self.identity_allocator)
                    .expect("文件管理器场景必须合法");
            controls = discover_controls(&tree);
        }
        if self.restore_focus_pending {
            restore_focus(&tree, &mut self.view_state);
            self.restore_focus_pending = false;
        }
        let modal_focus_changed = !ensure_modal_focus(&tree, &mut self.view_state).is_empty();
        let (search_focused, operation_focused) = self.input_focus_projection(&tree);
        let focus_projection_changed =
            props.search_focused != search_focused || props.operation_focused != operation_focused;
        if modal_focus_changed || focus_projection_changed {
            props.search_focused = search_focused;
            props.operation_focused = operation_focused;
            tree =
                UiTree::new_with_allocator(AppShell.render(&props), &mut self.identity_allocator)
                    .expect("文件管理器场景必须合法");
            controls = discover_controls(&tree);
        }
        self.finish_input_render();
        let frame = tree
            .resolve_dirty_with_focus(
                self.viewport,
                &DemoTextMeasurer,
                &scroll_inputs,
                &mut self.layout_cache,
                self.view_state.current_focus_key(),
                Some(FOCUS_APPEARANCE),
            )
            .expect("文件管理器场景必须可布局");
        self.frame_trace = crate::frame_trace::to_json(&frame).into_bytes();
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
    pub fn pointer_cursor(&self) -> u32 {
        if self.input_is_focused() {
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
        let changed = self.handle_ui_actions(&actions);
        if changed {
            self.mark_view_dirty();
        }
        actions.len() as u32
    }

    /// 应用当前键位快照解析原始按键后，才把语义意图交给 tela-core。
    ///
    /// 返回 1 表示组合键已被当前键位表消费，即使该意图最终没有产生业务动作；浏览器据此
    /// 阻止原生 Tab 等默认行为。
    pub fn handle_raw_key(&mut self, raw: RawKeyboardEvent) -> u32 {
        self.ensure_frame();
        if self.input_is_composing() {
            return 0;
        }
        let scopes = self
            .tree
            .as_ref()
            .expect("tree")
            .keymap_scopes_for_focus(self.view_state.current_focus_key());
        let Some(intent) = self.keymap.resolve(raw, &scopes) else {
            return 0;
        };
        let frame = self.frame().clone();
        let actions = handle_input(
            self.tree.as_ref().expect("tree"),
            &frame,
            &mut self.view_state,
            &InputEvent::Keyboard(intent),
        );
        if self.handle_ui_actions(&actions) {
            self.mark_view_dirty();
        }
        1
    }

    /// CPU/WASM 与 wasm-bindgen 共用的稳定原始键 ABI。
    pub fn handle_raw_key_codes(&mut self, code: u16, modifier_bits: u8, repeat: bool) -> u32 {
        raw_key_from_codes(code, modifier_bits, repeat)
            .map(|raw| self.handle_raw_key(raw))
            .unwrap_or(0)
    }

    /// 原子替换已校验的完整键位表；失败时保留旧快照。
    pub fn replace_keymap(&mut self, snapshot: KeymapSnapshot) -> Result<(), KeymapError> {
        snapshot.validate(Some(self.keymap.revision))?;
        self.keymap = snapshot;
        Ok(())
    }

    /// 浏览器/原生宿主的 JSON 注入入口。传输格式不进入 core 或 renderer。
    pub fn replace_keymap_json(&mut self, json: &str) -> Result<(), KeymapError> {
        self.replace_keymap(KeymapSnapshot::from_json(json)?)
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

    /// 为 CPU WASM ABI 分配键位表 JSON 上传缓冲区。
    pub fn begin_keymap_upload(&mut self, bytes: usize) -> *mut u8 {
        self.keymap_upload.resize(bytes, 0);
        self.keymap_upload.as_mut_ptr()
    }

    /// 校验并原子替换刚上传的键位表 JSON；失败时旧快照保持生效。
    pub fn finish_keymap_upload(&mut self, bytes: usize) -> u32 {
        if bytes != self.keymap_upload.len() {
            self.keymap_upload.clear();
            return 0;
        }
        let Ok(json) = String::from_utf8(std::mem::take(&mut self.keymap_upload)) else {
            return 0;
        };
        u32::from(self.replace_keymap_json(&json).is_ok())
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
    fn apply_controller_intent(&mut self, intent: Intent) {
        apply_intent(&mut self.model, &mut self.session, intent);
        if self.session.operation.is_none() {
            self.local_state
                .release_target(&tela_ui::IntentTarget::new("operation.value"));
        }
    }
    fn dispatch_controller_intent(&mut self, intent: Intent) {
        if matches!(intent, Intent::ConfirmOperation) {
            self.commit_operation_input_before_confirm();
        }
        self.apply_controller_intent(intent);
    }

    /// 统一消费 core 产生的 UI 生命周期动作。业务 mutation 只经应用意图写入。
    fn handle_ui_actions(&mut self, actions: &[UiAction]) -> bool {
        let mut changed = false;
        for action in actions {
            match action {
                UiAction::Click { node_id } => changed |= self.handle_click(*node_id),
                UiAction::Scroll { node_id, delta } => {
                    changed |= self.handle_scroll(*node_id, delta.y)
                }
                UiAction::Hover { node_id, entered } => {
                    let target = self.toolbar_target_at(*node_id);
                    if *entered {
                        if self.hovered_toolbar_target != target {
                            self.hovered_toolbar_target = target;
                            changed = true;
                        }
                    } else if target.as_deref() == self.hovered_toolbar_target.as_deref() {
                        self.hovered_toolbar_target = None;
                        changed = true;
                    }
                }
                UiAction::ShortcutActivated { shortcut_id } => {
                    changed |= self.handle_shortcut(shortcut_id);
                }
                UiAction::CloseModal { .. } if self.session.operation.is_some() => {
                    self.dispatch_controller_intent(Intent::CancelOperation);
                    self.restore_focus_pending = true;
                    changed = true;
                }
                UiAction::RequestFocus { .. } | UiAction::FocusChanged { .. } => changed = true,
                _ => {}
            }
        }
        changed
    }

    fn handle_shortcut(&mut self, shortcut: &ShortcutId) -> bool {
        match shortcut {
            ShortcutId::Undo if self.session.operation.is_none() => {
                self.dispatch_controller_intent(Intent::Command(crate::domain::FileCommand::Undo));
                true
            }
            _ => false,
        }
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

    /// 状态栏提示只投影当前 core hover key 在本帧树上的实际工具栏绑定。
    ///
    /// 节点条件卸载、重排或默认身份复用后，不能用旧字符串猜测它仍然对应哪个命令。
    fn toolbar_target_for_hover_key(&self, tree: &UiTree) -> Option<String> {
        let key = self.view_state.hover_key()?;
        tree.interact_for_key(key)
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
        let detail_h = (self.viewport.height
            - crate::presentation::shared::TOP_BAR_H
            - crate::presentation::shared::TOOLBAR_H
            - crate::presentation::shared::STATUS_BAR_H
            - crate::presentation::shared::DETAIL_HEADER_H)
            .max(80.0);
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
                tela_contract::DrawPayload::Text { text, .. } => Some(text.text.clone()),
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
                tela_contract::DrawPayload::Text { text, .. } if text.text == "TELA 文件"))
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
                    tela_contract::DrawPayload::Text { text, .. } if text.text == "README.md" => {
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
        app.handle_pointer(PointerEvent::Move {
            position: tela_contract::Point { x: -1.0, y: -1.0 },
        });
        assert_eq!(app.hovered_toolbar_target, None, "离开必须恢复状态栏投影");
        assert!(app.ensure_frame());
    }

    #[test]
    fn unloading_a_hovered_toolbar_node_clears_the_status_projection() {
        let mut app = App::new();
        app.session.select(5);
        app.invalidate_frame();
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
                    .is_some_and(|bind| bind.0 == "ui.invoke:command.rename")
            })
            .expect("选中项目后 Toolbar 重命名项应存在");
        let hit = app
            .frame()
            .hit_regions
            .iter()
            .find(|region| region.node_id == node_id)
            .expect("Toolbar 重命名项应可命中");
        app.handle_pointer(PointerEvent::Move {
            position: tela_contract::Point {
                x: hit.rect.x + 1.0,
                y: hit.rect.y + 1.0,
            },
        });
        assert_eq!(
            app.hovered_toolbar_target.as_deref(),
            Some("command.rename")
        );

        app.session.selected.clear();
        app.invalidate_frame();
        app.ensure_frame();
        assert_eq!(
            app.hovered_toolbar_target, None,
            "已卸载节点的 core hover key 不得继续投影旧状态栏说明"
        );
    }

    #[test]
    fn raw_keyboard_moves_default_focus_and_projects_a_focus_ring() {
        let mut app = App::new();
        app.ensure_frame();
        assert_eq!(
            app.handle_raw_key_codes(0x2b, 0, false),
            1,
            "Tab 应被默认键位表消费"
        );
        let first = app
            .view_state
            .current_focus_key()
            .cloned()
            .expect("Tab 后 core 应持有默认焦点");
        assert!(app.ensure_frame());
        assert!(app.frame().commands.iter().any(|command| {
            matches!(
                &command.payload,
                tela_contract::DrawPayload::RoundedRect {
                    fill: None,
                    border: Some(border),
                    ..
                } if border.color == FOCUS_APPEARANCE.color && border.width == FOCUS_APPEARANCE.width
            )
        }), "焦点变化必须在同一帧投影可见 FocusRing");

        assert_eq!(
            app.handle_raw_key_codes(0x51, 0, false),
            1,
            "ArrowDown 应被默认键位表消费"
        );
        let second = app
            .view_state
            .current_focus_key()
            .cloned()
            .expect("方向键后应仍有焦点");
        assert_ne!(
            first, second,
            "方向意图由焦点图/树序推进，而不是依赖页面手写 key"
        );
    }

    #[test]
    fn runtime_keymap_replacement_is_atomic_and_changes_the_next_key() {
        let mut app = App::new();
        app.ensure_frame();
        let replacement = r#"{
            "version": 1,
            "revision": 2,
            "default_layer": [
                {"key":"KeyA","intent":{"type":"focus_next"}}
            ]
        }"#;
        assert!(app.replace_keymap_json(replacement).is_ok());
        assert_eq!(
            app.handle_raw_key_codes(0x2b, 0, false),
            0,
            "旧 Tab 绑定不应残留"
        );
        assert_eq!(
            app.handle_raw_key_codes(0x04, 0, false),
            1,
            "新快照立即生效"
        );
        let focused = app.view_state.current_focus_key().cloned();
        assert!(focused.is_some());

        let invalid = r#"{
            "version": 1,
            "revision": 1,
            "default_layer": [
                {"key":"KeyB","intent":{"type":"focus_next"}}
            ]
        }"#;
        assert!(app.replace_keymap_json(invalid).is_err());
        assert_eq!(
            app.handle_raw_key_codes(0x04, 0, false),
            1,
            "拒绝快照后保留旧表"
        );
    }

    #[test]
    fn escape_closes_modal_and_restores_the_saved_background_focus() {
        let mut app = App::new();
        app.ensure_frame();
        assert_eq!(app.handle_raw_key_codes(0x2b, 0, false), 1);
        let background_focus = app.view_state.current_focus_key().cloned();
        assert!(app.dispatch_bind_id("command.new-folder"));
        assert!(app.ensure_frame());
        assert!(app.session.operation.is_some());
        let modal_focus = app.view_state.current_focus_key().cloned();
        assert_ne!(
            modal_focus, background_focus,
            "打开模态后 core 自动进入模态焦点域"
        );
        assert_eq!(
            app.handle_raw_key_codes(0x29, 0, false),
            1,
            "Escape 应进入 Cancel 意图"
        );
        assert!(app.session.operation.is_none(), "Cancel 动作关闭业务模态");
        assert!(app.ensure_frame());
        assert_eq!(
            app.view_state.current_focus_key(),
            background_focus.as_ref()
        );
    }

    #[test]
    fn tab_leaving_a_text_input_returns_arrow_keys_to_the_core_focus_graph() {
        let mut app = App::new();
        app.ensure_frame();
        assert_eq!(
            app.handle_raw_key_codes(0x2b, 0, false),
            1,
            "Tab 应进入搜索输入"
        );
        assert!(
            app.input_focused(),
            "当前 core 焦点是输入时才接管 DOM 文本编辑"
        );
        assert_eq!(app.input_focus(), 1, "DOM 焦点只记录 core 已判定的输入目标");

        assert_eq!(
            app.handle_raw_key_codes(0x2b, 0, false),
            1,
            "第二次 Tab 应离开输入"
        );
        assert!(
            !app.input_focused(),
            "弹窗或页面存在输入框不等于它仍拥有键盘方向键"
        );
        assert_eq!(app.input_blur(), 0, "无草稿时 DOM blur 不应产生业务写入");
        let after_tab = app
            .view_state
            .current_focus_key()
            .cloned()
            .expect("Tab 后应有下一个焦点目标");

        assert_eq!(
            app.handle_raw_key_codes(0x51, 0, false),
            1,
            "ArrowDown 应重新由默认键位表映射到 core"
        );
        assert_ne!(
            app.view_state.current_focus_key(),
            Some(&after_tab),
            "方向导航不能被已经失焦的隐藏 textarea 吞掉"
        );
    }

    #[test]
    fn modal_keymap_scope_overrides_the_default_snapshot_layer() {
        let mut app = App::new();
        app.ensure_frame();
        assert!(app.dispatch_bind_id("command.new-folder"));
        assert!(app.ensure_frame());
        assert!(
            app.operation_input_focused(),
            "默认模态焦点落在首个输入控件"
        );

        let replacement = r#"{
            "version": 1,
            "revision": 2,
            "default_layer": [
                {"key":"KeyA","intent":{"type":"focus_next"}}
            ],
            "scoped_layers": {
                "file-manager.operation": [
                    {"key":"KeyA","intent":{"type":"cancel"}}
                ]
            }
        }"#;
        assert!(app.replace_keymap_json(replacement).is_ok());
        assert_eq!(app.handle_raw_key_codes(0x04, 0, false), 1);
        assert!(
            app.session.operation.is_none(),
            "模态内层 KeymapScopeId 必须先于默认层命中"
        );
    }
}
