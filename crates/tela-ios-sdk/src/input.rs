//! Controlled whole-value text state for the UIKit software keyboard.

use tela_mobile_demo::MobileAppStatus;

/// Whether a status publication changed platform keyboard ownership.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextSync {
    /// Whether the native software keyboard must be shown or hidden.
    pub(crate) focus_changed: bool,
}

/// Mirrors the application's complete search value while UIKit owns text editing.
#[derive(Clone, Debug, Default)]
pub(crate) struct ControlledTextInput {
    focused: bool,
    value: String,
}

impl ControlledTextInput {
    /// Publishes the latest direct-mobile application state to the native text channel.
    pub(crate) fn publish(&mut self, status: MobileAppStatus) -> TextSync {
        let focus_changed = self.focused != status.input_focused;
        self.focused = status.input_focused;
        self.value = status.input_value;
        TextSync { focus_changed }
    }

    /// Appends a UIKit text insertion and returns the new controlled value.
    pub(crate) fn append(&mut self, text: &str) -> Option<String> {
        if !self.focused || text.is_empty() {
            return None;
        }
        self.value.push_str(text);
        Some(self.value.clone())
    }

    /// Removes one Unicode scalar for UIKit's `deleteBackward` event.
    pub(crate) fn delete_backward(&mut self) -> Option<String> {
        if !self.focused || self.value.pop().is_none() {
            return None;
        }
        Some(self.value.clone())
    }
}

#[cfg(test)]
mod tests {
    use tela_mobile_demo::MobileAppStatus;

    use super::ControlledTextInput;

    #[test]
    fn whole_value_edits_preserve_unicode_boundaries() {
        let mut input = ControlledTextInput::default();
        assert!(
            input
                .publish(MobileAppStatus {
                    input_focused: true,
                    input_value: "文件".to_owned(),
                })
                .focus_changed
        );
        assert_eq!(input.append("夹"), Some("文件夹".to_owned()));
        assert_eq!(input.delete_backward(), Some("文件".to_owned()));
    }

    #[test]
    fn blurred_input_rejects_platform_edits() {
        let mut input = ControlledTextInput::default();
        input.publish(MobileAppStatus::default());
        assert_eq!(input.append("x"), None);
        assert_eq!(input.delete_backward(), None);
    }
}
