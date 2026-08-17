//! Mobile application runtime: independent navigation, search, and Tela kernel composition.

use std::collections::HashMap;

use tela_contract::{
    BindId, FocusAppearance, InputEvent, Insets, NodeId, NodeKind, PhysicalKey, PointerEvent,
    ScrollState, SemanticKey, TextInputEvent, TextSelection, UiAction, UiFrame, UiNode,
    UiResources, Value, Viewport,
};
use tela_core::{DefaultApplicationProfile, IdentityAllocator, UiTree, ViewStateStore};
use tela_ui_headless::{
    ActionTrigger, ComponentPartPath, ComponentPartRole, EventFrame, EventRegistry, HeadlessEvent,
    RoutedEvent, components,
};

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
    resources: &'static dyn UiResources,
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
    event_registry: EventRegistry,
    event_frame: Option<EventFrame>,
}

#[cfg_attr(test, allow(dead_code))]
impl App {
    /// Creates the mobile browse session with product-selected visual resources.
    pub fn new(resources: &'static dyn UiResources) -> Self {
        Self {
            resources,
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
            event_registry: EventRegistry::new(),
            event_frame: None,
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
                self.resources.text_measurer(),
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
        self.event_registry = EventRegistry::new();
        register_mobile_event_routes(&mut self.event_registry, &tree);
        self.event_frame = Some(self.event_registry.begin_frame(&tree));
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
        if !self.input_focused() {
            if self.query == value {
                return 0;
            }
            self.query = value;
            self.reset_scroll();
            self.invalidate_frame();
            return 1;
        }
        u32::from(self.dispatch_text_input(TextInputEvent::Edit {
            selection: TextSelection::collapsed(value.len() as u32),
            value,
            composing: false,
        }))
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
        let committed = if self.input_focused() {
            let value = self.query.clone();
            self.dispatch_text_input(TextInputEvent::Commit {
                selection: TextSelection::collapsed(value.len() as u32),
                value,
            })
        } else {
            false
        };
        u32::from(committed || self.blur_input())
    }

    /// Cancels the current search interaction and clears its value.
    pub fn input_cancel(&mut self) -> u32 {
        let canceled = if self.input_focused() {
            self.dispatch_text_input(TextInputEvent::Cancel {
                selection: TextSelection::collapsed(self.query.len() as u32),
            })
        } else {
            let had_value = !self.query.is_empty();
            self.query.clear();
            had_value
        };
        let blurred = self.blur_input();
        if canceled || blurred {
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
            icons: self.resources.icon_provider(),
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
            let routed = self
                .event_frame
                .as_ref()
                .and_then(|frame| self.event_registry.dispatch(frame, action));
            if let Some(routed) = routed {
                changed |= self.handle_routed_event(routed);
                continue;
            }
            match action {
                UiAction::Scroll { node_id, delta } => {
                    changed |= self.handle_scroll(*node_id, delta.y)
                }
                UiAction::RequestFocus { .. } | UiAction::FocusChanged { .. } => changed = true,
                _ => {}
            }
        }
        changed
    }

    fn handle_routed_event(&mut self, routed: RoutedEvent) -> bool {
        match (routed.part.as_ref(), routed.event) {
            (Some(part), HeadlessEvent::Activate) if component_part_key(part) == "mobile.back" => {
                self.go_back()
            }
            (Some(part), HeadlessEvent::Activate | HeadlessEvent::Select { .. }) => {
                component_part_key(part)
                    .strip_prefix("mobile.entry.")
                    .is_some_and(|entry_id| self.open_entry(entry_id))
            }
            (
                Some(part),
                HeadlessEvent::TextInput {
                    event: TextInputEvent::Cancel { .. },
                },
            ) if component_part_key(part) == SEARCH_KEY => {
                if self.query.is_empty() {
                    false
                } else {
                    self.query.clear();
                    self.reset_scroll();
                    true
                }
            }
            (None, HeadlessEvent::ValueChange { bind_id, value }) => {
                self.handle_field_value_change(bind_id, value)
            }
            _ => false,
        }
    }

    fn handle_field_value_change(&mut self, bind_id: BindId, value: Value) -> bool {
        if bind_id.0 != SEARCH_KEY {
            return false;
        }
        let Value::String(value) = value else {
            return false;
        };
        if self.query == value {
            return false;
        }
        self.query = value;
        self.reset_scroll();
        true
    }

    fn dispatch_text_input(&mut self, event: TextInputEvent) -> bool {
        self.ensure_frame();
        let frame = self.frame().clone();
        let actions = self.profile.dispatch_input(
            self.tree.as_ref().expect("mobile tree must be ensured"),
            &frame,
            &mut self.view_state,
            &InputEvent::Text(event),
        );
        let changed = self.handle_actions(&actions);
        if changed {
            self.invalidate_frame();
        }
        changed
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
        self.event_frame = None;
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

fn register_mobile_event_routes(registry: &mut EventRegistry, tree: &UiTree) {
    for key in tree.keys() {
        let Some(interact) = tree.interact_for_key(key) else {
            continue;
        };
        if interact.input.is_some() {
            let root = components::Input::compose("mobile.search")
                .part(ComponentPartRole::Input, key.clone());
            let part = root.parts().last().expect("search input part");
            registry
                .register_part(
                    &root,
                    part,
                    ActionTrigger::TextInput,
                    HeadlessEvent::TextInput {
                        event: TextInputEvent::Cancel {
                            selection: TextSelection::default(),
                        },
                    },
                )
                .expect("search route must satisfy the Input contract");
        }
        if interact.clickable && interact.input.is_none() {
            if key.0.starts_with("mobile.entry.") {
                let root = components::List::compose("mobile.entries")
                    .part(ComponentPartRole::Item, key.clone());
                let part = root.parts().last().expect("entry item part");
                registry
                    .register_part(
                        &root,
                        part,
                        ActionTrigger::Click,
                        HeadlessEvent::Select {
                            value: key.0.clone(),
                        },
                    )
                    .expect("entry route must satisfy the List contract");
                continue;
            }

            let root = components::Button::compose("mobile.action")
                .part(ComponentPartRole::Trigger, key.clone());
            let part = root.parts().last().expect("action trigger part");
            registry
                .register_part(&root, part, ActionTrigger::Click, HeadlessEvent::Activate)
                .expect("action route must satisfy the Button contract");
        }
    }
}

fn component_part_key(part: &ComponentPartPath) -> &str {
    part.item_key().unwrap_or_else(|| part.as_str())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tela_contract::{
        Color, IconProvider, Insets, PhysicalKey, SemanticKey, UiAction, UiResources, Viewport,
    };
    use tela_core::{FocusSlot, UiTree};
    use tela_icon_resources::MaterialIconFontProvider;
    use tela_mobile_ui_kit::MobileRecipe;
    use tela_render_raster::{RasterConfig, render_frame};
    use tela_text_resources::ControlledTextMeasurer;
    use tela_ui_headless::{
        COMPONENT_CATALOG, ComponentArchetype, ComponentPartRole, ComponentRoot, ComponentState,
        ControlledValue,
    };

    use super::{App, Route};

    static TEST_TEXT_MEASURER: ControlledTextMeasurer = ControlledTextMeasurer;
    static TEST_ICON_PROVIDER: MaterialIconFontProvider = MaterialIconFontProvider;
    static TEST_RESOURCES: TestResources = TestResources;

    struct TestResources;

    impl UiResources for TestResources {
        fn text_measurer(&self) -> &dyn tela_contract::TextMeasurer {
            &TEST_TEXT_MEASURER
        }

        fn icon_provider(&self) -> &dyn IconProvider {
            &TEST_ICON_PROVIDER
        }
    }

    fn raster_fingerprint(pixels: &[u8]) -> u64 {
        pixels.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    fn catalog_matrix_value(root: &ComponentRoot, state: ComponentState) -> ControlledValue {
        match state {
            ComponentState::Content => ControlledValue::Text("matrix content".to_owned()),
            ComponentState::Value => match root.spec().archetype() {
                ComponentArchetype::Range => ControlledValue::Number(24.0),
                _ => ControlledValue::Text("matrix value".to_owned()),
            },
            ComponentState::Selection | ComponentState::Expanded => ControlledValue::Keys(vec![
                root.parts()
                    .iter()
                    .find(|part| part.role() == ComponentPartRole::Item)
                    .expect("stateful collection must expose an item")
                    .key()
                    .0
                    .clone(),
            ]),
            ComponentState::Open | ComponentState::Disabled | ComponentState::Loading => {
                ControlledValue::Bool(true)
            }
            ComponentState::Items => {
                ControlledValue::Keys(vec!["matrix alpha".to_owned(), "matrix beta".to_owned()])
            }
            ComponentState::Query => ControlledValue::Text("matrix query".to_owned()),
            ComponentState::Range => ControlledValue::Number(42.0),
            ComponentState::CurrentPage => ControlledValue::Number(2.0),
            ComponentState::Progress => ControlledValue::Number(48.0),
            ComponentState::Error => ControlledValue::Text("matrix error".to_owned()),
        }
    }

    fn catalog_state_context(root: ComponentRoot, state: ComponentState) -> ComponentRoot {
        if root.spec().archetype() == ComponentArchetype::Layer && state != ComponentState::Open {
            root.state(ComponentState::Open, ControlledValue::Bool(true))
        } else {
            root
        }
    }

    fn catalog_fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(1_099_511_628_211);
        }
        hash
    }

    #[test]
    fn search_is_controlled_and_system_back_clears_it_before_navigation() {
        let mut app = App::new(&TEST_RESOURCES);
        assert_eq!(app.set_input_value("架构".to_owned()), 1);
        assert_eq!(app.visible_entries().len(), 1);
        assert!(app.go_back());
        assert!(app.query.is_empty());
        assert_eq!(app.route, Route::Browse(None));
    }

    #[test]
    fn focused_search_routes_kernel_text_events_through_field_value_change() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.ensure_frame());
        let key = SemanticKey(super::SEARCH_KEY.to_owned());
        let node_id = app
            .tree
            .as_ref()
            .expect("tree")
            .node_id_for_key(&key)
            .expect("search field must have a stable key");
        app.view_state.set_current_focus(FocusSlot {
            key: Some(key),
            node_id: Some(node_id),
        });

        assert_eq!(app.set_input_value("架构".to_owned()), 1);
        assert_eq!(app.input_value(), "架构");
        assert_eq!(app.input_cancel(), 1);
        assert_eq!(app.input_value(), "");
    }

    #[test]
    fn escape_at_root_is_unhandled_so_the_android_host_can_finish() {
        let mut app = App::new(&TEST_RESOURCES);
        assert_eq!(app.handle_key(PhysicalKey::Escape as u16), 0);
    }

    #[test]
    fn frame_uses_the_concrete_default_application_profile() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.ensure_frame());
        assert!(!app.frame().commands.is_empty());
    }

    #[test]
    fn safe_area_mobile_screen_has_a_stable_raster_reference() {
        let mut app = App::new(&TEST_RESOURCES);
        app.set_viewport(390.0, 844.0);
        assert!(app.set_safe_area(Insets {
            top: 47.0,
            right: 0.0,
            bottom: 34.0,
            left: 0.0,
        }));
        assert!(app.ensure_frame());

        let bitmap = render_frame(
            app.frame(),
            &RasterConfig::default_with(Color::rgba(1.0, 1.0, 1.0, 1.0)),
        );
        assert_eq!((bitmap.width, bitmap.height), (390, 844));
        assert!(
            bitmap
                .pixels
                .chunks_exact(4)
                .any(|pixel| pixel != [255, 255, 255, 255]),
            "移动业务视图不能退化为空白 raster"
        );

        // FNV-1a 指纹是完整 RGBA 缓冲的紧凑 golden reference；视觉改动必须显式更新它。
        assert_eq!(
            raster_fingerprint(&bitmap.pixels),
            4_663_901_108_784_084_735
        );
    }

    #[test]
    fn mobile_catalog_raster_reference_is_stable_at_the_application_boundary() {
        let viewport = Viewport {
            width: 240.0,
            height: 360.0,
        };
        let background = Color::rgba(1.0, 1.0, 1.0, 1.0);
        let mut hash = 14_695_981_039_346_656_037_u64;
        for spec in COMPONENT_CATALOG {
            if !spec.contract().recipes.mobile {
                continue;
            }
            for state in
                std::iter::once(None).chain(spec.contract().states.iter().copied().map(Some))
            {
                let root = match state {
                    None => spec.root(format!("reference.raster.mobile.{}", spec.name)),
                    Some(state) => {
                        let baseline = catalog_state_context(
                            spec.root(format!("reference.raster.mobile.{}.{state:?}", spec.name)),
                            state,
                        );
                        baseline
                            .clone()
                            .state(state, catalog_matrix_value(&baseline, state))
                    }
                };
                let tree = UiTree::new(MobileRecipe::new(&root).into_node().unwrap_or_else(
                    |error| panic!("{} {state:?} raster recipe: {error:?}", spec.name),
                ))
                .unwrap_or_else(|error| panic!("{} {state:?} raster tree: {error:?}", spec.name));
                let frame = tree
                    .resolve(viewport, &TEST_TEXT_MEASURER, &HashMap::new())
                    .unwrap_or_else(|error| {
                        panic!("{} {state:?} raster frame: {error:?}", spec.name)
                    });
                let bitmap = render_frame(&frame, &RasterConfig::default_with(background));
                assert_eq!(
                    (bitmap.width, bitmap.height),
                    (viewport.width as u32, viewport.height as u32),
                    "{} {state:?} raster dimensions",
                    spec.name
                );
                assert!(
                    bitmap
                        .pixels
                        .chunks_exact(4)
                        .any(|pixel| pixel != [255, 255, 255, 255]),
                    "{} {state:?} must not produce a blank raster",
                    spec.name
                );
                hash = catalog_fnv1a(hash, spec.name.as_bytes());
                hash = catalog_fnv1a(hash, format!("{state:?}").as_bytes());
                hash = catalog_fnv1a(hash, &bitmap.pixels);
            }
        }
        assert_eq!(
            hash, 17_379_567_066_821_406_475,
            "update the raster reference intentionally after visual review"
        );
    }

    #[test]
    fn mobile_cell_routes_its_semantic_action_through_the_event_frame() {
        let mut app = App::new(&TEST_RESOURCES);
        assert!(app.ensure_frame());
        let node_id = app
            .tree
            .as_ref()
            .expect("tree")
            .node_id_for_key(&SemanticKey("mobile.entry.design".to_owned()))
            .expect("the first MobileCell must expose its semantic action key");

        assert!(app.handle_actions(&[UiAction::Click { node_id }]));
        assert_eq!(app.route, Route::Browse(Some("design".to_owned())));
    }

    #[test]
    fn safe_area_is_normalized_and_invalidates_the_projection() {
        let mut app = App::new(&TEST_RESOURCES);
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
