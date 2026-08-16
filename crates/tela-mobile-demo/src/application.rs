//! Mobile application runtime: independent navigation, search, and Tela kernel composition.

use std::collections::HashMap;

use tela_contract::{
    FocusAppearance, InputEvent, Insets, NodeId, NodeKind, PhysicalKey, PointerEvent, ScrollState,
    SemanticKey, UiAction, UiFrame, UiNode, Viewport,
};
use tela_core::{DefaultApplicationProfile, IdentityAllocator, UiTree, ViewStateStore};
use tela_text::ControlledTextMeasurer;

use crate::{
    domain::{Entry, EntryKind, MobileWorkspace},
    presentation::{MobileViewProps, render},
};

/// Initial mobile logical size before a target host reports its real content area.
pub const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 360.0,
    height: 720.0,
};

const SEARCH_KEY: &str = "mobile.search";
const FOCUS_APPEARANCE: FocusAppearance = FocusAppearance {
    color: tela_contract::Color::rgba(0.145, 0.388, 0.922, 1.0),
    width: 2.0,
    inset: 2.0,
};

#[cfg_attr(test, allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Route {
    Browse(Option<String>),
    Preview(String),
}

/// A complete mobile application session. It does not share desktop page or domain state.
pub struct App {
    workspace: MobileWorkspace,
    route: Route,
    history: Vec<Route>,
    query: String,
    viewport: Viewport,
    safe_area: Insets,
    frame: Option<UiFrame>,
    tree: Option<UiTree>,
    profile: DefaultApplicationProfile,
    identity_allocator: IdentityAllocator,
    view_state: ViewStateStore,
    scroll_key: Option<SemanticKey>,
}

#[cfg_attr(test, allow(dead_code))]
impl App {
    /// Creates the mobile browse session at the workspace root.
    pub fn new() -> Self {
        Self {
            workspace: MobileWorkspace::sample(),
            route: Route::Browse(None),
            history: Vec::new(),
            query: String::new(),
            viewport: DEFAULT_VIEWPORT,
            safe_area: Insets::all(0.0),
            frame: None,
            tree: None,
            profile: DefaultApplicationProfile::new(),
            identity_allocator: IdentityAllocator::new(),
            view_state: ViewStateStore::new(),
            scroll_key: None,
        }
    }

    /// Updates the logical mobile content area.
    pub fn set_viewport(&mut self, width: f32, height: f32) -> bool {
        let viewport = Viewport {
            width: width.max(240.0),
            height: height.max(320.0),
        };
        if self.viewport == viewport {
            return false;
        }
        self.viewport = viewport;
        self.invalidate_frame();
        true
    }

    /// Updates the native system-bar exclusion area expressed in logical pixels.
    #[cfg(any(test, feature = "native-app"))]
    pub fn set_safe_area(&mut self, safe_area: Insets) -> bool {
        let safe_area = Insets {
            top: safe_area.top.max(0.0),
            right: safe_area.right.max(0.0),
            bottom: safe_area.bottom.max(0.0),
            left: safe_area.left.max(0.0),
        };
        if self.safe_area == safe_area {
            return false;
        }
        self.safe_area = safe_area;
        self.invalidate_frame();
        true
    }

    /// Ensures the current mobile projection and frame exist.
    pub fn ensure_frame(&mut self) -> bool {
        if self.frame.is_some() {
            return false;
        }
        let focused_before = self.input_focused();
        let mut tree = self.build_tree(focused_before);
        self.profile.reconcile_tree(&tree, &mut self.view_state);
        self.profile.ensure_modal_focus(&tree, &mut self.view_state);
        let focused_after = self.input_focused();
        if focused_before != focused_after {
            tree = self.build_tree(focused_after);
            self.profile.reconcile_tree(&tree, &mut self.view_state);
            self.profile.ensure_modal_focus(&tree, &mut self.view_state);
        }
        let scroll_inputs = self.active_scroll_inputs();
        let frame = self
            .profile
            .resolve(
                &tree,
                self.viewport,
                &ControlledTextMeasurer,
                &scroll_inputs,
                &self.view_state,
                Some(FOCUS_APPEARANCE),
            )
            .expect("mobile scene must be valid");
        if self.clamp_scroll_states(&frame) {
            self.invalidate_frame();
            return self.ensure_frame();
        }
        self.scroll_key = discover_scroll_key(&tree);
        self.tree = Some(tree);
        self.frame = Some(frame);
        true
    }

    /// Returns the resolved Tela frame for the current mobile screen.
    pub fn frame(&self) -> &UiFrame {
        self.frame.as_ref().expect("mobile frame must be ensured")
    }

    /// Delivers a normalized pointer event from a mobile target adapter.
    pub fn handle_pointer(&mut self, event: PointerEvent) -> u32 {
        self.ensure_frame();
        let frame = self.frame().clone();
        let tree = self.tree.as_ref().expect("mobile tree must be ensured");
        let actions = self.profile.dispatch_input(
            tree,
            &frame,
            &mut self.view_state,
            &InputEvent::Pointer(event),
        );
        let changed = self.handle_actions(&actions);
        if changed {
            self.invalidate_frame();
        }
        actions.len() as u32
    }

    /// Handles the small platform key vocabulary used by mobile target adapters.
    pub fn handle_key(&mut self, physical_key: u16) -> u32 {
        match PhysicalKey::from_code(physical_key) {
            Some(PhysicalKey::Escape) => u32::from(self.go_back()),
            Some(PhysicalKey::Enter) if self.input_focused() => u32::from(self.blur_input()),
            _ => 0,
        }
    }

    /// Replaces the controlled query value supplied by the native text channel.
    pub fn set_input_value(&mut self, value: String) -> u32 {
        if self.query == value {
            return 0;
        }
        self.query = value;
        self.reset_scroll();
        self.invalidate_frame();
        1
    }

    /// The platform text channel became focused. The core click already owns focus state.
    pub fn input_focus(&mut self) -> u32 {
        u32::from(self.input_focused())
    }

    /// The platform text channel lost focus.
    pub fn input_blur(&mut self) -> u32 {
        u32::from(self.blur_input())
    }

    /// Commits the current search interaction by hiding the text channel but retaining the query.
    pub fn input_enter(&mut self) -> u32 {
        u32::from(self.blur_input())
    }

    /// Cancels the current search interaction and clears its value.
    pub fn input_cancel(&mut self) -> u32 {
        let had_value = !self.query.is_empty();
        self.query.clear();
        let blurred = self.blur_input();
        if had_value || blurred {
            self.reset_scroll();
            self.invalidate_frame();
            1
        } else {
            0
        }
    }

    /// Composition markers are accepted so the host can preserve the ABI contract. The complete
    /// controlled value arrives separately through [`Self::set_input_value`].
    pub fn composition_changed(&mut self) -> u32 {
        u32::from(self.input_focused())
    }

    /// Whether the native text channel should be attached.
    pub fn input_focused(&self) -> bool {
        self.view_state
            .current_focus_key()
            .is_some_and(|key| key.0 == SEARCH_KEY)
    }

    /// Current controlled search value.
    pub fn input_value(&self) -> String {
        self.query.clone()
    }

    fn build_tree(&mut self, search_focused: bool) -> UiTree {
        let title = self.title().to_owned();
        let entries = self.visible_entries();
        let preview = self.preview_entry();
        let root = render(MobileViewProps {
            viewport: self.viewport,
            title: &title,
            can_go_back: self.can_go_back(),
            query: &self.query,
            search_focused,
            safe_area: self.safe_area,
            entries,
            preview,
        });
        UiTree::new_with_allocator(root, &mut self.identity_allocator)
            .expect("mobile view must construct a valid tree")
    }

    fn title(&self) -> &str {
        match &self.route {
            Route::Browse(folder) => self.workspace.folder_title(folder.as_deref()),
            Route::Preview(entry) => self
                .workspace
                .entry(entry)
                .map(|entry| entry.name)
                .unwrap_or("文件预览"),
        }
    }

    fn visible_entries(&self) -> Vec<&Entry> {
        if !self.query.trim().is_empty() {
            return self.workspace.search(&self.query);
        }
        match &self.route {
            Route::Browse(folder) => self.workspace.children(folder.as_deref()),
            Route::Preview(_) => Vec::new(),
        }
    }

    fn preview_entry(&self) -> Option<&Entry> {
        match &self.route {
            Route::Preview(id) => self.workspace.entry(id),
            Route::Browse(_) => None,
        }
    }

    fn can_go_back(&self) -> bool {
        !self.query.is_empty() || !self.history.is_empty()
    }

    fn handle_actions(&mut self, actions: &[UiAction]) -> bool {
        let mut changed = false;
        for action in actions {
            match action {
                UiAction::Click { node_id } => changed |= self.handle_click(*node_id),
                UiAction::Scroll { node_id, delta } => {
                    changed |= self.handle_scroll(*node_id, delta.y)
                }
                UiAction::RequestFocus { .. } | UiAction::FocusChanged { .. } => changed = true,
                _ => {}
            }
        }
        changed
    }

    fn handle_click(&mut self, node_id: NodeId) -> bool {
        let bind_id = self
            .tree
            .as_ref()
            .and_then(|tree| bind_id_at(tree, node_id))
            .map(str::to_owned);
        let Some(bind_id) = bind_id else {
            return false;
        };
        if bind_id == "mobile.back" {
            return self.go_back();
        }
        if bind_id == SEARCH_KEY {
            return true;
        }
        let Some(entry_id) = bind_id.strip_prefix("mobile.entry.") else {
            return false;
        };
        self.open_entry(entry_id)
    }

    fn open_entry(&mut self, entry_id: &str) -> bool {
        let Some(entry) = self.workspace.entry(entry_id) else {
            return false;
        };
        let next = match entry.kind {
            EntryKind::Folder => {
                self.query.clear();
                Route::Browse(Some(entry.id.to_owned()))
            }
            EntryKind::Document | EntryKind::Asset => Route::Preview(entry.id.to_owned()),
        };
        self.history.push(self.route.clone());
        self.route = next;
        self.reset_scroll();
        true
    }

    fn go_back(&mut self) -> bool {
        if !self.query.is_empty() {
            self.query.clear();
            self.reset_scroll();
            self.invalidate_frame();
            return true;
        }
        let Some(previous) = self.history.pop() else {
            return false;
        };
        self.route = previous;
        self.reset_scroll();
        self.invalidate_frame();
        true
    }

    fn blur_input(&mut self) -> bool {
        if !self.input_focused() {
            return false;
        }
        self.view_state.clear_current_focus();
        self.invalidate_frame();
        true
    }

    fn handle_scroll(&mut self, node_id: NodeId, delta_y: f32) -> bool {
        let Some(bounds) = self.frame.as_ref().and_then(|frame| {
            frame
                .scroll_bounds
                .iter()
                .find(|bounds| bounds.node_id == node_id)
        }) else {
            return false;
        };
        let mut state = self.view_state.scroll(&bounds.key);
        let next = (state.offset_y + delta_y).clamp(0.0, bounds.max_offset_y);
        if (next - state.offset_y).abs() < f32::EPSILON {
            return false;
        }
        state.offset_y = next;
        self.view_state.set_scroll(bounds.key.clone(), state);
        true
    }

    fn active_scroll_inputs(&self) -> HashMap<SemanticKey, ScrollState> {
        self.scroll_key
            .as_ref()
            .map(|key| (key.clone(), self.view_state.scroll(key)))
            .into_iter()
            .collect()
    }

    fn clamp_scroll_states(&mut self, frame: &UiFrame) -> bool {
        let mut changed = false;
        for bounds in &frame.scroll_bounds {
            let state = self.view_state.scroll(&bounds.key);
            let next = ScrollState {
                offset_x: state.offset_x.clamp(0.0, bounds.max_offset_x),
                offset_y: state.offset_y.clamp(0.0, bounds.max_offset_y),
            };
            if next != state {
                self.view_state.set_scroll(bounds.key.clone(), next);
                changed = true;
            }
        }
        changed
    }

    fn reset_scroll(&mut self) {
        if let Some(key) = self.scroll_key.clone() {
            self.view_state.set_scroll(key, ScrollState::default());
        }
    }

    fn invalidate_frame(&mut self) {
        self.frame = None;
        self.tree = None;
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

fn discover_scroll_key(tree: &UiTree) -> Option<SemanticKey> {
    fn visit(node: &UiNode, keys: &[SemanticKey], index: &mut usize) -> Option<SemanticKey> {
        let key = keys.get(*index).cloned();
        *index += 1;
        if matches!(
            node.kind,
            NodeKind::ScrollView | NodeKind::VirtualListView(_)
        ) {
            return key;
        }
        for child in &node.children {
            if let Some(key) = visit(child, keys, index) {
                return Some(key);
            }
        }
        None
    }
    visit(tree.root(), tree.keys(), &mut 0)
}

#[cfg_attr(test, allow(dead_code))]
fn bind_id_at(tree: &UiTree, target: NodeId) -> Option<&str> {
    fn visit<'a>(node: &'a UiNode, target: NodeId, index: &mut u32) -> Option<&'a str> {
        if *index == target.0 {
            return node
                .interact
                .as_ref()
                .and_then(|interact| interact.bind_id.as_ref())
                .map(|bind| bind.0.as_str());
        }
        *index += 1;
        for child in &node.children {
            if let Some(bind) = visit(child, target, index) {
                return Some(bind);
            }
        }
        None
    }
    visit(tree.root(), target, &mut 0)
}

#[cfg(test)]
mod tests {
    use tela_contract::{Insets, PhysicalKey};

    use super::{App, Route};

    #[test]
    fn search_is_controlled_and_system_back_clears_it_before_navigation() {
        let mut app = App::new();
        assert_eq!(app.set_input_value("架构".to_owned()), 1);
        assert_eq!(app.visible_entries().len(), 1);
        assert!(app.go_back());
        assert!(app.query.is_empty());
        assert_eq!(app.route, Route::Browse(None));
    }

    #[test]
    fn escape_at_root_is_unhandled_so_the_android_host_can_finish() {
        let mut app = App::new();
        assert_eq!(app.handle_key(PhysicalKey::Escape as u16), 0);
    }

    #[test]
    fn frame_uses_the_concrete_default_application_profile() {
        let mut app = App::new();
        assert!(app.ensure_frame());
        assert!(!app.frame().commands.is_empty());
    }

    #[test]
    fn safe_area_is_normalized_and_invalidates_the_projection() {
        let mut app = App::new();
        assert!(app.ensure_frame());
        assert!(app.set_safe_area(Insets {
            top: 47.0,
            right: -2.0,
            bottom: 34.0,
            left: -1.0,
        }));
        assert_eq!(
            app.safe_area,
            Insets {
                top: 47.0,
                right: 0.0,
                bottom: 34.0,
                left: 0.0,
            }
        );
        assert!(app.ensure_frame());
        assert!(!app.set_safe_area(app.safe_area));
    }
}
