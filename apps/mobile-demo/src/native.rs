//! Direct Rust adapter for mobile targets that statically link the application.

use tela_contract::{Insets, PointerEvent, UiFrame, UiResources};

use crate::application::App;

/// State a native host needs to synchronize its controlled text channel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MobileAppStatus {
    /// Whether the platform text channel should own native focus.
    pub input_focused: bool,
    /// The full controlled search value.
    pub input_value: String,
}

/// A concrete mobile application session for a statically linked target host.
///
/// This intentionally exposes direct Rust calls instead of reusing the WASM packet ABI. The
/// application remains independent from every target's window, renderer, lifecycle, and input
/// implementation.
pub struct MobileApp {
    app: App,
}

impl MobileApp {
    /// Starts a new mobile file-browser session with product-selected visual resources.
    pub fn new(resources: &'static dyn UiResources) -> Self {
        Self {
            app: App::new(resources),
        }
    }

    /// Updates the target's full logical viewport.
    pub fn set_viewport(&mut self, width: f32, height: f32) -> bool {
        self.app.set_viewport(width, height)
    }

    /// Updates the logical area excluded by system chrome and gesture affordances.
    pub fn set_safe_area(&mut self, safe_area: Insets) -> bool {
        self.app.set_safe_area(safe_area)
    }

    /// Delivers a normalized touch or scroll event from a frame the target actually presented.
    ///
    /// `frame_token` must be the value returned with that frame from [`Self::frame`]. A stale
    /// surface event is rejected before Kernel hit testing or DSL action routing.
    pub fn dispatch_pointer(&mut self, frame_token: u64, event: PointerEvent) -> bool {
        self.app.handle_pointer_for_frame(frame_token, event) > 0
    }

    /// Delivers a platform-neutral physical key code from a presented frame.
    pub fn dispatch_key(&mut self, frame_token: u64, physical_key: u16) -> bool {
        self.app.handle_key_for_frame(frame_token, physical_key) != 0
    }

    /// Replaces the complete controlled text value from the native keyboard's presented frame.
    pub fn set_input_value(&mut self, frame_token: u64, value: String) -> bool {
        self.app.set_input_value_for_frame(frame_token, value) != 0
    }

    /// Records an IME composition update for the native editor attached to a presented frame.
    pub fn composition_changed(&mut self, frame_token: u64) -> bool {
        self.app.composition_changed_for_frame(frame_token) != 0
    }

    /// Confirms native text focus for a presented frame.
    pub fn input_focus(&mut self, frame_token: u64) -> bool {
        self.app.input_focus_for_frame(frame_token) != 0
    }

    /// Reports native text blur for a presented frame.
    pub fn input_blur(&mut self, frame_token: u64) -> bool {
        self.app.input_blur_for_frame(frame_token) != 0
    }

    /// Commits the current text interaction for a presented frame.
    pub fn input_enter(&mut self, frame_token: u64) -> bool {
        self.app.input_enter_for_frame(frame_token) != 0
    }

    /// Cancels the current text interaction for a presented frame.
    pub fn input_cancel(&mut self, frame_token: u64) -> bool {
        self.app.input_cancel_for_frame(frame_token) != 0
    }

    /// Resolves the portable drawing frame and its nonzero provenance token together.
    ///
    /// A target must save the returned token only after it successfully presents this frame. It
    /// must then attach that saved token to every later input event sampled from the surface.
    pub fn frame(&mut self) -> (&UiFrame, u64) {
        self.app.ensure_frame();
        (self.app.frame(), self.app.active_frame_token())
    }

    /// Returns the state needed by a native controlled text channel.
    pub fn status(&self) -> MobileAppStatus {
        MobileAppStatus {
            input_focused: self.app.input_focused(),
            input_value: self.app.input_value(),
        }
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{IconProvider, Insets, Point, PointerEvent, UiResources};
    use tela_icon_resources::MaterialIconFontProvider;
    use tela_text_resources::ControlledTextMeasurer;

    use super::MobileApp;

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

    #[test]
    fn native_session_publishes_the_same_controlled_input_state() {
        let mut app = MobileApp::new(&TEST_RESOURCES);
        let (_, token) = app.frame();
        assert!(!app.set_input_value(token, "未聚焦".to_owned()));
        assert!(app.dispatch_pointer(token, PointerEvent::mouse_down(Point { x: 24.0, y: 88.0 })));
        let (_, token) = app.frame();
        assert!(app.status().input_focused);
        assert!(app.set_input_value(token, "文件".to_owned()));
        assert_eq!(app.status().input_value, "文件");
        assert!(!app.frame().0.commands.is_empty());
    }

    #[test]
    fn native_session_accepts_safe_area_and_pointer_events() {
        let mut app = MobileApp::new(&TEST_RESOURCES);
        assert!(app.set_safe_area(Insets {
            top: 48.0,
            right: 0.0,
            bottom: 34.0,
            left: 0.0,
        }));
        let (_, token) = app.frame();
        let _ = app.dispatch_pointer(token, PointerEvent::mouse_move(Point { x: 20.0, y: 60.0 }));
        assert!(!app.frame().0.commands.is_empty());
    }

    #[test]
    fn native_session_rejects_input_from_a_replaced_presented_frame() {
        let mut app = MobileApp::new(&TEST_RESOURCES);
        let (_, initial_token) = app.frame();
        assert!(app.dispatch_pointer(
            initial_token,
            PointerEvent::mouse_down(Point { x: 24.0, y: 88.0 })
        ));
        let (_, first_token) = app.frame();
        assert!(app.set_input_value(first_token, "架构".to_owned()));

        let (_, second_token) = app.frame();
        assert_ne!(first_token, second_token);
        assert!(!app.set_input_value(first_token, "stale".to_owned()));
        assert_eq!(app.status().input_value, "架构");
    }
}
