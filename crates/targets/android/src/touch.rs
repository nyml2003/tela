//! Single-pointer touch normalization for a touch-first Tela guest.

/// Android touch phases represented without a Winit dependency so they can be tested locally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchPhase {
    /// A finger became the active pointer.
    Started,
    /// The active finger moved.
    Moved,
    /// The active finger was released.
    Ended,
    /// Android cancelled the active gesture.
    Cancelled,
}

/// A normalized guest pointer action emitted by the touch adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum GuestPointerEvent {
    /// Delayed press emitted only after a gesture remains a tap.
    Down { x: f32, y: f32 },
    /// Tap release emitted immediately after the corresponding delayed press.
    Up { x: f32, y: f32 },
    /// Content-space scroll delta. Positive `delta_y` advances content downward.
    Scroll {
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
    },
}

#[derive(Clone, Copy, Debug)]
struct ActiveTouch {
    id: u64,
    start_x: f32,
    start_y: f32,
    last_x: f32,
    last_y: f32,
    dragging: bool,
}

/// Defers guest clicks until Android's touch slop distinguishes a tap from a scroll.
#[derive(Clone, Debug)]
pub(crate) struct TouchAdapter {
    touch_slop: f32,
    active: Option<ActiveTouch>,
}

impl TouchAdapter {
    /// Creates an adapter using touch slop in the host's chosen coordinate space.
    pub(crate) fn new(touch_slop: f32) -> Self {
        Self {
            touch_slop: touch_slop.max(0.0),
            active: None,
        }
    }

    /// Updates touch slop when the host's coordinate transform changes.
    pub(crate) fn set_touch_slop(&mut self, touch_slop: f32) {
        self.touch_slop = touch_slop.max(0.0);
    }

    /// Accepts one physical touch point and yields only the guest events it should observe.
    pub(crate) fn handle(
        &mut self,
        id: u64,
        phase: TouchPhase,
        x: f32,
        y: f32,
    ) -> Vec<GuestPointerEvent> {
        match phase {
            TouchPhase::Started => self.start(id, x, y),
            TouchPhase::Moved => self.move_active(id, x, y),
            TouchPhase::Ended => self.end(id, x, y),
            TouchPhase::Cancelled => self.cancel(id),
        }
    }

    fn start(&mut self, id: u64, x: f32, y: f32) -> Vec<GuestPointerEvent> {
        if self.active.is_some() {
            return Vec::new();
        }
        self.active = Some(ActiveTouch {
            id,
            start_x: x,
            start_y: y,
            last_x: x,
            last_y: y,
            dragging: false,
        });
        Vec::new()
    }

    fn move_active(&mut self, id: u64, x: f32, y: f32) -> Vec<GuestPointerEvent> {
        let Some(mut active) = self.active else {
            return Vec::new();
        };
        if active.id != id {
            return Vec::new();
        }
        let previous_x = active.last_x;
        let previous_y = active.last_y;
        active.last_x = x;
        active.last_y = y;
        if !active.dragging {
            let delta_x = x - active.start_x;
            let delta_y = y - active.start_y;
            active.dragging = delta_x.hypot(delta_y) >= self.touch_slop;
        }
        self.active = Some(active);
        if !active.dragging {
            return Vec::new();
        }
        let delta_x = previous_x - x;
        let delta_y = previous_y - y;
        if delta_x.abs() < f32::EPSILON && delta_y.abs() < f32::EPSILON {
            return Vec::new();
        }
        vec![GuestPointerEvent::Scroll {
            x,
            y,
            delta_x,
            delta_y,
        }]
    }

    fn end(&mut self, id: u64, x: f32, y: f32) -> Vec<GuestPointerEvent> {
        let Some(active) = self.active.take() else {
            return Vec::new();
        };
        if active.id != id {
            self.active = Some(active);
            return Vec::new();
        }
        if active.dragging {
            Vec::new()
        } else {
            vec![
                GuestPointerEvent::Down { x, y },
                GuestPointerEvent::Up { x, y },
            ]
        }
    }

    fn cancel(&mut self, id: u64) -> Vec<GuestPointerEvent> {
        if self.active.is_some_and(|active| active.id == id) {
            self.active = None;
        }
        Vec::new()
    }
}

/// Converts Winit's physical touch coordinate into the Guest's logical coordinate space.
pub(crate) fn logical_coordinate(physical: f64, scale_factor: f64) -> f32 {
    (physical / scale_factor.max(f64::EPSILON)) as f32
}

#[cfg(test)]
mod tests {
    use super::{GuestPointerEvent, TouchAdapter, TouchPhase, logical_coordinate};

    #[test]
    fn physical_touch_coordinates_follow_the_guest_logical_viewport() {
        assert_eq!(logical_coordinate(240.0, 2.0), 120.0);
        assert_eq!(logical_coordinate(240.0, 1.5), 160.0);
    }

    #[test]
    fn tap_defers_down_until_the_pointer_is_released() {
        let mut adapter = TouchAdapter::new(12.0);
        assert!(
            adapter
                .handle(7, TouchPhase::Started, 20.0, 30.0)
                .is_empty()
        );
        assert!(adapter.handle(7, TouchPhase::Moved, 24.0, 33.0).is_empty());
        assert_eq!(
            adapter.handle(7, TouchPhase::Ended, 24.0, 33.0),
            [
                GuestPointerEvent::Down { x: 24.0, y: 33.0 },
                GuestPointerEvent::Up { x: 24.0, y: 33.0 },
            ]
        );
    }

    #[test]
    fn drag_becomes_scroll_without_a_guest_click() {
        let mut adapter = TouchAdapter::new(8.0);
        let _ = adapter.handle(3, TouchPhase::Started, 100.0, 100.0);
        assert_eq!(
            adapter.handle(3, TouchPhase::Moved, 100.0, 124.0),
            [GuestPointerEvent::Scroll {
                x: 100.0,
                y: 124.0,
                delta_x: 0.0,
                delta_y: -24.0,
            }]
        );
        assert!(
            adapter
                .handle(3, TouchPhase::Ended, 100.0, 140.0)
                .is_empty()
        );
    }

    #[test]
    fn secondary_pointer_cannot_steal_the_active_gesture() {
        let mut adapter = TouchAdapter::new(8.0);
        let _ = adapter.handle(1, TouchPhase::Started, 0.0, 0.0);
        assert!(adapter.handle(2, TouchPhase::Started, 0.0, 0.0).is_empty());
        assert!(adapter.handle(2, TouchPhase::Ended, 0.0, 0.0).is_empty());
        assert_eq!(adapter.handle(1, TouchPhase::Ended, 0.0, 0.0).len(), 2);
    }

    #[test]
    fn cancellation_and_density_changes_leave_no_stale_click() {
        let mut adapter = TouchAdapter::new(8.0);
        adapter.set_touch_slop(16.0);
        let _ = adapter.handle(1, TouchPhase::Started, 0.0, 0.0);
        assert!(
            adapter
                .handle(1, TouchPhase::Cancelled, 4.0, 4.0)
                .is_empty()
        );
        assert!(adapter.handle(1, TouchPhase::Ended, 4.0, 4.0).is_empty());
    }
}
