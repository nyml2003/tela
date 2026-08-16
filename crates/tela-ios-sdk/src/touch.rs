//! iPhone touch normalization for the direct mobile session.

use tela_contract::{Point, PointerEvent};

/// UIKit touch phases represented independently from Winit for local unit tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchPhase {
    /// A finger became the active pointer.
    Started,
    /// The active finger moved.
    Moved,
    /// The active finger was released.
    Ended,
    /// UIKit cancelled the active gesture.
    Cancelled,
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

/// Keeps taps atomic while turning sufficiently long movement into a mobile scroll gesture.
#[derive(Clone, Debug)]
pub(crate) struct TouchAdapter {
    touch_slop: f32,
    active: Option<ActiveTouch>,
}

impl TouchAdapter {
    /// Creates an adapter using a touch slop measured in UIKit logical points.
    pub(crate) fn new(touch_slop: f32) -> Self {
        Self {
            touch_slop: touch_slop.max(0.0),
            active: None,
        }
    }

    /// Clears an interrupted gesture before the next UIKit lifecycle activation.
    pub(crate) fn reset(&mut self) {
        self.active = None;
    }

    /// Accepts one logical UIKit touch point and emits direct application pointer events.
    pub(crate) fn handle(
        &mut self,
        id: u64,
        phase: TouchPhase,
        x: f32,
        y: f32,
    ) -> Vec<PointerEvent> {
        match phase {
            TouchPhase::Started => self.start(id, x, y),
            TouchPhase::Moved => self.move_active(id, x, y),
            TouchPhase::Ended => self.end(id, x, y),
            TouchPhase::Cancelled => self.cancel(id),
        }
    }

    fn start(&mut self, id: u64, x: f32, y: f32) -> Vec<PointerEvent> {
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

    fn move_active(&mut self, id: u64, x: f32, y: f32) -> Vec<PointerEvent> {
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
            active.dragging = (x - active.start_x).hypot(y - active.start_y) >= self.touch_slop;
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
        vec![PointerEvent::Scroll {
            position: Point { x, y },
            delta: Point {
                x: delta_x,
                y: delta_y,
            },
        }]
    }

    fn end(&mut self, id: u64, x: f32, y: f32) -> Vec<PointerEvent> {
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
            let position = Point { x, y };
            vec![
                PointerEvent::Down { position },
                PointerEvent::Up { position },
            ]
        }
    }

    fn cancel(&mut self, id: u64) -> Vec<PointerEvent> {
        if self.active.is_some_and(|active| active.id == id) {
            self.active = None;
        }
        Vec::new()
    }
}

/// Converts Winit's physical touch coordinates into UIKit's logical point space.
pub(crate) fn logical_coordinate(physical: f64, scale_factor: f64) -> f32 {
    (physical / scale_factor.max(f64::EPSILON)) as f32
}

#[cfg(test)]
mod tests {
    use tela_contract::{Point, PointerEvent};

    use super::{TouchAdapter, TouchPhase, logical_coordinate};

    #[test]
    fn physical_touch_coordinates_follow_uikit_points() {
        assert_eq!(logical_coordinate(240.0, 2.0), 120.0);
    }

    #[test]
    fn tap_waits_for_release() {
        let mut adapter = TouchAdapter::new(12.0);
        assert!(
            adapter
                .handle(7, TouchPhase::Started, 20.0, 30.0)
                .is_empty()
        );
        assert_eq!(
            adapter.handle(7, TouchPhase::Ended, 20.0, 30.0),
            [
                PointerEvent::Down {
                    position: Point { x: 20.0, y: 30.0 },
                },
                PointerEvent::Up {
                    position: Point { x: 20.0, y: 30.0 },
                },
            ]
        );
    }

    #[test]
    fn drag_scrolls_without_clicking() {
        let mut adapter = TouchAdapter::new(8.0);
        let _ = adapter.handle(3, TouchPhase::Started, 100.0, 100.0);
        assert_eq!(
            adapter.handle(3, TouchPhase::Moved, 100.0, 124.0),
            [PointerEvent::Scroll {
                position: Point { x: 100.0, y: 124.0 },
                delta: Point { x: 0.0, y: -24.0 },
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
    fn cancellation_clears_a_pending_tap() {
        let mut adapter = TouchAdapter::new(8.0);
        let _ = adapter.handle(1, TouchPhase::Started, 0.0, 0.0);
        assert!(
            adapter
                .handle(1, TouchPhase::Cancelled, 0.0, 0.0)
                .is_empty()
        );
        let _ = adapter.handle(1, TouchPhase::Started, 0.0, 0.0);
        adapter.reset();
        assert!(adapter.handle(1, TouchPhase::Ended, 0.0, 0.0).is_empty());
    }
}
