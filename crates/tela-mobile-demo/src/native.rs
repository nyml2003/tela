//! Direct Rust adapter for mobile targets that statically link the application.

use tela_contract::{Insets, PointerEvent, UiFrame};

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
    /// Starts a new mobile file-browser session.
    pub fn new() -> Self {
        Self { app: App::new() }
    }

    /// Updates the target's full logical viewport.
    pub fn set_viewport(&mut self, width: f32, height: f32) -> bool {
        self.app.set_viewport(width, height)
    }

    /// Updates the logical area excluded by system chrome and gesture affordances.
    pub fn set_safe_area(&mut self, safe_area: Insets) -> bool {
        self.app.set_safe_area(safe_area)
    }

    /// Delivers a normalized touch or scroll event.
    pub fn dispatch_pointer(&mut self, event: PointerEvent) -> bool {
        self.app.handle_pointer(event) > 0
    }

    /// Delivers a platform-neutral physical key code.
    pub fn dispatch_key(&mut self, physical_key: u16) -> bool {
        self.app.handle_key(physical_key) != 0
    }

    /// Replaces the complete controlled text value from the native keyboard.
    pub fn set_input_value(&mut self, value: String) -> bool {
        self.app.set_input_value(value) != 0
    }

    /// Records an IME composition update while preserving the controlled text value.
    pub fn composition_changed(&mut self) -> bool {
        self.app.composition_changed() != 0
    }

    /// Confirms native text focus.
    pub fn input_focus(&mut self) -> bool {
        self.app.input_focus() != 0
    }

    /// Reports native text blur.
    pub fn input_blur(&mut self) -> bool {
        self.app.input_blur() != 0
    }

    /// Commits the current text interaction.
    pub fn input_enter(&mut self) -> bool {
        self.app.input_enter() != 0
    }

    /// Cancels the current text interaction.
    pub fn input_cancel(&mut self) -> bool {
        self.app.input_cancel() != 0
    }

    /// Resolves and borrows the current portable drawing frame.
    pub fn frame(&mut self) -> &UiFrame {
        self.app.ensure_frame();
        self.app.frame()
    }

    /// Returns the state needed by a native controlled text channel.
    pub fn status(&self) -> MobileAppStatus {
        MobileAppStatus {
            input_focused: self.app.input_focused(),
            input_value: self.app.input_value(),
        }
    }
}

impl Default for MobileApp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use tela_contract::{Insets, Point, PointerEvent};

    use super::MobileApp;

    #[test]
    fn native_session_publishes_the_same_controlled_input_state() {
        let mut app = MobileApp::new();
        assert!(app.set_input_value("文件".to_owned()));
        assert_eq!(app.status().input_value, "文件");
        assert!(!app.frame().commands.is_empty());
    }

    #[test]
    fn native_session_accepts_safe_area_and_pointer_events() {
        let mut app = MobileApp::new();
        assert!(app.set_safe_area(Insets {
            top: 48.0,
            right: 0.0,
            bottom: 34.0,
            left: 0.0,
        }));
        let _ = app.dispatch_pointer(PointerEvent::Move {
            position: Point { x: 20.0, y: 60.0 },
        });
        assert!(!app.frame().commands.is_empty());
    }
}
