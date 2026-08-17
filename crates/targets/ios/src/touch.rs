//! iPhone touch normalization without target-side gesture recognition.

use std::{collections::BTreeMap, time::Instant};

use tela_contract::{Point, PointerButtons, PointerEvent, PointerId, PointerKind, PointerPhase};

/// UIKit touch phases represented independently from Winit for local unit tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchPhase {
    /// A finger entered the target surface.
    Started,
    /// A finger changed coordinates.
    Moved,
    /// A finger left normally.
    Ended,
    /// UIKit cancelled the pointer sequence.
    Cancelled,
}

/// Normalizes UIKit data into raw Contract pointer events.
///
/// No touch slop, delayed click, or scroll synthesis exists here. The active map is only lifecycle
/// bookkeeping so a suspended UIKit host can send one `Cancel` for each touch still known to it.
#[derive(Clone, Debug)]
pub(crate) struct TouchAdapter {
    epoch: Instant,
    active: BTreeMap<u64, Point>,
}

impl TouchAdapter {
    /// Creates an empty raw touch normalizer.
    pub(crate) fn new() -> Self {
        Self {
            epoch: Instant::now(),
            active: BTreeMap::new(),
        }
    }

    /// Converts one logical UIKit touch point using the host monotonic clock.
    pub(crate) fn handle(&mut self, id: u64, phase: TouchPhase, x: f32, y: f32) -> PointerEvent {
        self.handle_at(id, phase, x, y, self.now_micros())
    }

    /// Converts one logical UIKit touch point using an explicit timestamp for tests.
    pub(crate) fn handle_at(
        &mut self,
        id: u64,
        phase: TouchPhase,
        x: f32,
        y: f32,
        timestamp_micros: u64,
    ) -> PointerEvent {
        let position = Point { x, y };
        let phase = match phase {
            TouchPhase::Started => {
                self.active.insert(id, position);
                PointerPhase::Down
            }
            TouchPhase::Moved => {
                self.active.insert(id, position);
                PointerPhase::Move
            }
            TouchPhase::Ended => {
                self.active.remove(&id);
                PointerPhase::Up
            }
            TouchPhase::Cancelled => {
                self.active.remove(&id);
                PointerPhase::Cancel
            }
        };
        PointerEvent::new(
            PointerId(id),
            PointerKind::Touch,
            phase,
            position,
            if matches!(phase, PointerPhase::Up | PointerPhase::Cancel) {
                PointerButtons::NONE
            } else {
                PointerButtons::PRIMARY
            },
            timestamp_micros,
            Point { x: 0.0, y: 0.0 },
        )
    }

    /// Converts every active touch into a terminal cancellation during lifecycle suspension.
    pub(crate) fn cancel_all(&mut self) -> Vec<PointerEvent> {
        let timestamp_micros = self.now_micros();
        std::mem::take(&mut self.active)
            .into_iter()
            .map(|(id, position)| {
                PointerEvent::new(
                    PointerId(id),
                    PointerKind::Touch,
                    PointerPhase::Cancel,
                    position,
                    PointerButtons::NONE,
                    timestamp_micros,
                    Point { x: 0.0, y: 0.0 },
                )
            })
            .collect()
    }

    fn now_micros(&self) -> u64 {
        self.epoch.elapsed().as_micros().min(u128::from(u64::MAX)) as u64
    }
}

impl Default for TouchAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts Winit's physical touch coordinates into UIKit's logical point space.
pub(crate) fn logical_coordinate(physical: f64, scale_factor: f64) -> f32 {
    (physical / scale_factor.max(f64::EPSILON)) as f32
}

#[cfg(test)]
mod tests {
    use tela_contract::{Point, PointerButtons, PointerKind, PointerPhase};

    use super::{TouchAdapter, TouchPhase, logical_coordinate};

    #[test]
    fn physical_touch_coordinates_follow_uikit_points() {
        assert_eq!(logical_coordinate(240.0, 2.0), 120.0);
    }

    #[test]
    fn each_touch_phase_is_delivered_as_the_same_raw_sequence() {
        let mut adapter = TouchAdapter::new();
        let down = adapter.handle_at(7, TouchPhase::Started, 20.0, 30.0, 10);
        let moved = adapter.handle_at(7, TouchPhase::Moved, 24.0, 33.0, 20);
        let up = adapter.handle_at(7, TouchPhase::Ended, 24.0, 33.0, 30);
        assert_eq!(down.kind, PointerKind::Touch);
        assert_eq!(down.phase, PointerPhase::Down);
        assert_eq!(down.buttons, PointerButtons::PRIMARY);
        assert_eq!(moved.phase, PointerPhase::Move);
        assert_eq!(up.phase, PointerPhase::Up);
        assert_eq!(up.buttons, PointerButtons::NONE);
        assert_eq!(moved.position, Point { x: 24.0, y: 33.0 });
        assert_eq!(
            adapter.handle(8, TouchPhase::Cancelled, 1.0, 2.0).phase,
            PointerPhase::Cancel
        );
    }

    #[test]
    fn simultaneous_touches_survive_and_lifecycle_reset_cancels_each_one() {
        let mut adapter = TouchAdapter::new();
        let _ = adapter.handle_at(1, TouchPhase::Started, 10.0, 20.0, 10);
        let _ = adapter.handle_at(2, TouchPhase::Started, 30.0, 40.0, 11);
        let _ = adapter.handle_at(1, TouchPhase::Moved, 12.0, 22.0, 12);
        let cancelled = adapter.cancel_all();
        assert_eq!(cancelled.len(), 2);
        assert_eq!(cancelled[0].pointer_id.0, 1);
        assert_eq!(cancelled[0].position, Point { x: 12.0, y: 22.0 });
        assert_eq!(cancelled[1].pointer_id.0, 2);
        assert!(
            cancelled
                .iter()
                .all(|event| event.phase == PointerPhase::Cancel)
        );
    }
}
