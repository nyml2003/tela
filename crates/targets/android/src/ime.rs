//! Controlled whole-value text synchronization for the Android native text channel.

/// The guest-visible state mirrored by Kotlin's native `EditText`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextInputState {
    /// Whether the host should attach and focus the native text channel.
    pub(crate) focused: bool,
    /// The full controlled value, always synchronized as one UTF-8 string.
    pub(crate) value: String,
}

/// Maintains the small, deliberately limited IME contract for the first Android release.
#[derive(Clone, Debug, Default)]
pub(crate) struct ControlledTextSync {
    state: TextInputState,
    composing: bool,
}

impl ControlledTextSync {
    /// Replaces the host mirror with the latest guest status.
    pub(crate) fn publish_guest(&mut self, focused: bool, value: &str) -> bool {
        let next = TextInputState {
            focused,
            value: value.to_owned(),
        };
        if self.state == next {
            return false;
        }
        self.state = next;
        true
    }

    /// Optimistically records a native whole-value edit until the guest publishes it back.
    pub(crate) fn accept_native_value(&mut self, value: String) -> bool {
        if self.state.value == value {
            return false;
        }
        self.state.value = value;
        true
    }

    /// Begins a composition segment. Repeated markers are intentionally coalesced.
    pub(crate) fn begin_composition(&mut self) -> bool {
        if self.composing {
            return false;
        }
        self.composing = true;
        true
    }

    /// Ends a composition segment. Repeated markers are intentionally coalesced.
    pub(crate) fn end_composition(&mut self) -> bool {
        if !self.composing {
            return false;
        }
        self.composing = false;
        true
    }

    /// Returns a copy suitable for a JNI caller on Android's UI thread.
    pub(crate) fn snapshot(&self) -> TextInputState {
        self.state.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::ControlledTextSync;

    #[test]
    fn guest_and_native_always_exchange_the_complete_value() {
        let mut sync = ControlledTextSync::default();
        assert!(sync.publish_guest(true, "架构"));
        assert_eq!(sync.snapshot().value, "架构");
        assert!(sync.accept_native_value("架构迭代".to_owned()));
        assert_eq!(sync.snapshot().value, "架构迭代");
        assert!(!sync.accept_native_value("架构迭代".to_owned()));
    }

    #[test]
    fn composition_markers_are_balanced_without_affecting_the_value() {
        let mut sync = ControlledTextSync::default();
        assert!(sync.begin_composition());
        assert!(!sync.begin_composition());
        assert!(sync.end_composition());
        assert!(!sync.end_composition());
        assert!(sync.snapshot().value.is_empty());
    }
}
