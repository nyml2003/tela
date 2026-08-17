//! Android touch normalization without target-side click or scroll recognition.

use std::{collections::BTreeMap, time::Instant};

use tela_app_abi::{AppPointerEvent, AppPointerKind, AppPointerPhase};

/// Android touch phases represented without a Winit dependency so they can be tested locally.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchPhase {
    /// A finger entered the target surface.
    Started,
    /// A finger changed coordinates.
    Moved,
    /// A finger left normally.
    Ended,
    /// Android cancelled the pointer sequence.
    Cancelled,
}

/// Normalizes Android touch coordinates, phase and lifecycle cancellation into raw ABI packets.
///
/// It deliberately retains no tap/drag threshold and never turns movement into `Scroll`. The
/// small active-point map exists solely to issue `Cancel` packets when the Android surface is
/// suspended before Winit can send terminal phases.
#[derive(Clone, Debug)]
pub(crate) struct TouchAdapter {
    epoch: Instant,
    active: BTreeMap<u64, (f32, f32)>,
}

impl TouchAdapter {
    /// Creates an empty raw touch normalizer.
    pub(crate) fn new() -> Self {
        Self {
            epoch: Instant::now(),
            active: BTreeMap::new(),
        }
    }

    /// Converts one Android touch point using the host monotonic clock.
    pub(crate) fn handle(&mut self, id: u64, phase: TouchPhase, x: f32, y: f32) -> AppPointerEvent {
        self.handle_at(id, phase, x, y, self.now_micros())
    }

    /// Converts one Android touch point using an explicit monotonic timestamp for tests.
    pub(crate) fn handle_at(
        &mut self,
        id: u64,
        phase: TouchPhase,
        x: f32,
        y: f32,
        timestamp_micros: u64,
    ) -> AppPointerEvent {
        let phase = match phase {
            TouchPhase::Started => {
                self.active.insert(id, (x, y));
                AppPointerPhase::Down
            }
            TouchPhase::Moved => {
                self.active.insert(id, (x, y));
                AppPointerPhase::Move
            }
            TouchPhase::Ended => {
                self.active.remove(&id);
                AppPointerPhase::Up
            }
            TouchPhase::Cancelled => {
                self.active.remove(&id);
                AppPointerPhase::Cancel
            }
        };
        AppPointerEvent::new(
            id,
            AppPointerKind::Touch,
            phase,
            x,
            y,
            if matches!(phase, AppPointerPhase::Up | AppPointerPhase::Cancel) {
                0
            } else {
                1
            },
            timestamp_micros,
            0.0,
            0.0,
        )
    }

    /// Emits terminal packets for every active touch before a lifecycle suspension.
    pub(crate) fn cancel_all(&mut self) -> Vec<AppPointerEvent> {
        let timestamp_micros = self.now_micros();
        std::mem::take(&mut self.active)
            .into_iter()
            .map(|(id, (x, y))| {
                AppPointerEvent::new(
                    id,
                    AppPointerKind::Touch,
                    AppPointerPhase::Cancel,
                    x,
                    y,
                    0,
                    timestamp_micros,
                    0.0,
                    0.0,
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

/// Converts Winit's physical touch coordinate into the guest's logical coordinate space.
pub(crate) fn logical_coordinate(physical: f64, scale_factor: f64) -> f32 {
    (physical / scale_factor.max(f64::EPSILON)) as f32
}

#[cfg(test)]
mod tests {
    use tela_app_abi::{AppPointerKind, AppPointerPhase};

    use super::{TouchAdapter, TouchPhase, logical_coordinate};

    #[test]
    fn physical_touch_coordinates_follow_the_guest_logical_viewport() {
        assert_eq!(logical_coordinate(240.0, 2.0), 120.0);
        assert_eq!(logical_coordinate(240.0, 1.5), 160.0);
    }

    #[test]
    fn every_touch_phase_is_forwarded_immediately_without_gesture_preclassification() {
        let mut adapter = TouchAdapter::new();
        let down = adapter.handle_at(7, TouchPhase::Started, 20.0, 30.0, 10);
        let moved = adapter.handle_at(7, TouchPhase::Moved, 24.0, 33.0, 20);
        let up = adapter.handle_at(7, TouchPhase::Ended, 24.0, 33.0, 30);
        assert_eq!(down.kind, AppPointerKind::Touch);
        assert_eq!(down.phase, AppPointerPhase::Down);
        assert_eq!(moved.phase, AppPointerPhase::Move);
        assert_eq!(up.phase, AppPointerPhase::Up);
        assert_eq!(moved.pointer_id, 7);
        assert_eq!((moved.x, moved.y, moved.timestamp_micros), (24.0, 33.0, 20));
        assert_eq!(
            adapter.handle(8, TouchPhase::Cancelled, 1.0, 2.0).phase,
            AppPointerPhase::Cancel
        );
    }

    #[test]
    fn simultaneous_pointers_are_preserved_and_lifecycle_reset_cancels_each_one() {
        let mut adapter = TouchAdapter::new();
        let _ = adapter.handle_at(1, TouchPhase::Started, 10.0, 20.0, 10);
        let _ = adapter.handle_at(2, TouchPhase::Started, 30.0, 40.0, 11);
        let _ = adapter.handle_at(1, TouchPhase::Moved, 12.0, 22.0, 12);
        let cancelled = adapter.cancel_all();
        assert_eq!(cancelled.len(), 2);
        assert_eq!(cancelled[0].pointer_id, 1);
        assert_eq!((cancelled[0].x, cancelled[0].y), (12.0, 22.0));
        assert_eq!(cancelled[1].pointer_id, 2);
        assert!(
            cancelled
                .iter()
                .all(|event| event.phase == AppPointerPhase::Cancel)
        );
    }
}
