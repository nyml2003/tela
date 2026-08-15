//! Pure lifecycle policy for the Win32 development shell.
//!
//! Native resources and Win32 messages stay in `win32.rs`. This module only decides which state
//! transitions are legal, so the non-Windows test target can exercise the shell contract without
//! creating an HWND or GPU device.

/// A shell-visible phase that owns the meaning of redraw and input requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShellPhase {
    /// The window is visible while bundle loading and guest initialization happen off the UI thread.
    Loading,
    /// A guest, a non-zero client area, and a renderable GPU session are available.
    Running,
    /// The guest is ready, but the client area is currently zero-sized or minimized.
    Suspended,
    /// Startup failed and the native shell is showing its diagnostic page.
    Failed,
    /// The HWND is being torn down and must not accept late work.
    Closing,
}

/// One edge of the native text-editor channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextChannelAction {
    /// Attach the native text channel to the guest-selected input target.
    Focus,
    /// Detach the native text channel and let the guest commit its local draft once.
    Blur,
}

/// How the shell responds to a device-lost callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeviceLossAction {
    /// Recreate the complete GPU session once on the UI thread.
    RecreateGpu,
    /// Stop instead of entering an unbounded device-loss loop.
    Exit,
}

/// State shared conceptually by the native message pump and its background startup worker.
///
/// It deliberately carries no HWND, guest, WGPU object, timer handle, or application state. Those
/// values remain owned by the Win32 shell; this type only makes their ordering explicit.
#[derive(Debug)]
pub(crate) struct ShellLifecycle {
    phase: ShellPhase,
    redraw_pending: bool,
    retry_timer_pending: bool,
    retry_attempt: u8,
    device_recovery_available: bool,
    window_focused: bool,
    text_channel_attached: bool,
}

impl Default for ShellLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellLifecycle {
    /// Starts before the window is shown, while no guest or GPU resource exists yet.
    pub(crate) fn new() -> Self {
        Self {
            phase: ShellPhase::Loading,
            redraw_pending: false,
            retry_timer_pending: false,
            retry_attempt: 0,
            device_recovery_available: true,
            window_focused: false,
            text_channel_attached: false,
        }
    }

    /// Current shell phase.
    pub(crate) fn phase(&self) -> ShellPhase {
        self.phase
    }

    /// Whether drawing a guest frame is currently valid.
    pub(crate) fn can_render(&self) -> bool {
        self.phase == ShellPhase::Running
    }

    /// Coalesces invalidations. The caller only asks Win32 to repaint when this returns `true`.
    pub(crate) fn request_redraw(&mut self) -> bool {
        if !matches!(
            self.phase,
            ShellPhase::Loading | ShellPhase::Running | ShellPhase::Failed
        ) || self.redraw_pending
        {
            return false;
        }
        self.redraw_pending = true;
        true
    }

    /// Marks the pending invalidation as being serviced by `WM_PAINT`.
    pub(crate) fn begin_paint(&mut self) {
        self.redraw_pending = false;
    }

    /// Accepts a background startup result. Late results after close are intentionally ignored.
    pub(crate) fn startup_succeeded(&mut self, client_available: bool) {
        if self.phase != ShellPhase::Loading {
            return;
        }
        self.phase = if client_available {
            ShellPhase::Running
        } else {
            ShellPhase::Suspended
        };
    }

    /// Moves the startup page to a native error page. It never revives a closing shell.
    pub(crate) fn startup_failed(&mut self) {
        if self.phase == ShellPhase::Loading {
            self.phase = ShellPhase::Failed;
        }
    }

    /// Applies a zero/non-zero client-area transition after the guest exists.
    pub(crate) fn client_area_changed(&mut self, client_available: bool) {
        match (self.phase, client_available) {
            (ShellPhase::Running, false) => self.phase = ShellPhase::Suspended,
            (ShellPhase::Suspended, true) => self.phase = ShellPhase::Running,
            _ => {}
        }
    }

    /// Schedules one bounded retry after an acquire timeout.
    pub(crate) fn surface_timeout(&mut self) -> Option<u32> {
        if !self.can_render() || self.retry_timer_pending {
            return None;
        }
        const RETRY_DELAYS_MS: [u32; 5] = [16, 32, 64, 128, 250];
        let index = usize::from(self.retry_attempt).min(RETRY_DELAYS_MS.len() - 1);
        self.retry_attempt = self.retry_attempt.saturating_add(1);
        self.retry_timer_pending = true;
        Some(RETRY_DELAYS_MS[index])
    }

    /// Consumes the one-shot retry timer. A suspended or closing window does not repaint.
    pub(crate) fn take_surface_retry(&mut self) -> bool {
        if !self.retry_timer_pending {
            return false;
        }
        self.retry_timer_pending = false;
        self.can_render()
    }

    /// A frame made forward progress, so the next timeout returns to the lowest retry delay.
    pub(crate) fn surface_presented(&mut self) {
        self.retry_attempt = 0;
        self.retry_timer_pending = false;
    }

    /// Discards a retry made obsolete by resize, suspension, or teardown.
    pub(crate) fn cancel_surface_retry(&mut self) {
        self.retry_timer_pending = false;
    }

    /// Allows exactly one complete GPU reconstruction for this process lifetime.
    pub(crate) fn device_lost(&mut self) -> Option<DeviceLossAction> {
        if !matches!(self.phase, ShellPhase::Running | ShellPhase::Suspended) {
            return None;
        }
        if self.device_recovery_available {
            self.device_recovery_available = false;
            Some(DeviceLossAction::RecreateGpu)
        } else {
            Some(DeviceLossAction::Exit)
        }
    }

    /// Records native window focus and returns a text-channel edge when one is required.
    pub(crate) fn set_window_focus(
        &mut self,
        window_focused: bool,
        guest_wants_text: bool,
    ) -> Option<TextChannelAction> {
        self.window_focused = window_focused;
        self.reconcile_text_channel(guest_wants_text)
    }

    /// Reconciles guest focus after a guest event changes its current focus key.
    pub(crate) fn reconcile_text_channel(
        &mut self,
        guest_wants_text: bool,
    ) -> Option<TextChannelAction> {
        let should_attach = self.can_render() && self.window_focused && guest_wants_text;
        if should_attach == self.text_channel_attached {
            return None;
        }
        self.text_channel_attached = should_attach;
        Some(if should_attach {
            TextChannelAction::Focus
        } else {
            TextChannelAction::Blur
        })
    }

    /// Stops accepting work and detaches the text channel at most once.
    pub(crate) fn begin_close(&mut self) -> Option<TextChannelAction> {
        if self.phase == ShellPhase::Closing {
            return None;
        }
        self.phase = ShellPhase::Closing;
        self.redraw_pending = false;
        self.retry_timer_pending = false;
        self.window_focused = false;
        if self.text_channel_attached {
            self.text_channel_attached = false;
            Some(TextChannelAction::Blur)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_waits_for_a_non_zero_client_area() {
        let mut lifecycle = ShellLifecycle::new();
        lifecycle.startup_succeeded(false);
        assert_eq!(lifecycle.phase(), ShellPhase::Suspended);
        assert!(!lifecycle.can_render());

        lifecycle.client_area_changed(true);
        assert_eq!(lifecycle.phase(), ShellPhase::Running);
        assert!(lifecycle.can_render());
    }

    #[test]
    fn closing_rejects_late_startup_results() {
        let mut lifecycle = ShellLifecycle::new();
        lifecycle.begin_close();
        lifecycle.startup_succeeded(true);
        lifecycle.startup_failed();
        assert_eq!(lifecycle.phase(), ShellPhase::Closing);
    }

    #[test]
    fn redraws_are_coalesced_until_paint_begins() {
        let mut lifecycle = ShellLifecycle::new();
        assert!(lifecycle.request_redraw());
        assert!(!lifecycle.request_redraw());
        lifecycle.begin_paint();
        assert!(lifecycle.request_redraw());

        lifecycle.startup_succeeded(false);
        lifecycle.begin_paint();
        assert!(!lifecycle.request_redraw());
    }

    #[test]
    fn surface_timeout_uses_one_timer_and_resets_after_present() {
        let mut lifecycle = ShellLifecycle::new();
        lifecycle.startup_succeeded(true);
        assert_eq!(lifecycle.surface_timeout(), Some(16));
        assert_eq!(lifecycle.surface_timeout(), None);
        assert!(lifecycle.take_surface_retry());
        assert_eq!(lifecycle.surface_timeout(), Some(32));
        lifecycle.cancel_surface_retry();
        assert!(!lifecycle.take_surface_retry());
        assert_eq!(lifecycle.surface_timeout(), Some(64));
        assert!(lifecycle.take_surface_retry());
        lifecycle.surface_presented();
        assert_eq!(lifecycle.surface_timeout(), Some(16));
    }

    #[test]
    fn device_loss_has_one_recovery_budget() {
        let mut lifecycle = ShellLifecycle::new();
        lifecycle.startup_succeeded(true);
        assert_eq!(lifecycle.device_lost(), Some(DeviceLossAction::RecreateGpu));
        assert_eq!(lifecycle.device_lost(), Some(DeviceLossAction::Exit));
    }

    #[test]
    fn device_loss_while_suspended_defers_rebuild_until_the_client_returns() {
        let mut lifecycle = ShellLifecycle::new();
        lifecycle.startup_succeeded(true);
        lifecycle.client_area_changed(false);
        assert_eq!(lifecycle.phase(), ShellPhase::Suspended);
        assert_eq!(lifecycle.device_lost(), Some(DeviceLossAction::RecreateGpu));
    }

    #[test]
    fn native_focus_edges_attach_and_detach_text_once() {
        let mut lifecycle = ShellLifecycle::new();
        lifecycle.startup_succeeded(true);
        assert_eq!(lifecycle.set_window_focus(true, false), None);
        assert_eq!(
            lifecycle.reconcile_text_channel(true),
            Some(TextChannelAction::Focus)
        );
        assert_eq!(lifecycle.reconcile_text_channel(true), None);
        assert_eq!(
            lifecycle.set_window_focus(false, true),
            Some(TextChannelAction::Blur)
        );
        assert_eq!(lifecycle.begin_close(), None);
    }
}
