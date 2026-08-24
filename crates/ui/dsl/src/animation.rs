//! Guest 侧确定性动画时钟、easing 与可重定向插值控制器。
//!
//! 平台宿主只注入单调时间戳；本模块不读取系统时钟，也不依赖 renderer。相同目标值和
//! 时间戳序列始终产生相同样本。

use tela_contract::{
    BorderRadius, Color, ColorStop, Fill, Gradient, GradientKind, PixelOffset, Point, ShadowSpec,
    VisualConcern,
};

/// Host 注入的一次单调时钟采样。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnimationClock {
    /// 单调毫秒刻度。
    pub timestamp_ms: u64,
}

/// 一帧动画对 Host 的后续调度请求。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnimationSchedule {
    /// 是否仍有未完成的动画。
    pub active: bool,
    /// 最早的下一唤醒时间。
    pub next_deadline_ms: Option<u64>,
}

impl AnimationSchedule {
    /// 合并一个组件的调度请求。
    pub fn merge(&mut self, other: Self) {
        self.active |= other.active;
        self.next_deadline_ms = match (self.next_deadline_ms, other.next_deadline_ms) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left @ Some(_), None) | (None, left @ Some(_)) => left,
            (None, None) => None,
        };
    }
}

/// 缓动曲线。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    /// 匀速插值。
    Linear,
    /// CSS 兼容三次贝塞尔曲线；x 控制点会限制到 `0..=1`。
    CubicBezier {
        /// 第一个控制点 x。
        x1: f32,
        /// 第一个控制点 y。
        y1: f32,
        /// 第二个控制点 x。
        x2: f32,
        /// 第二个控制点 y。
        y2: f32,
    },
}

impl Easing {
    /// 常用的自然减速曲线，等价于 CSS `cubic-bezier(.2, 0, 0, 1)`。
    pub const STANDARD: Self = Self::CubicBezier {
        x1: 0.2,
        y1: 0.0,
        x2: 0.0,
        y2: 1.0,
    };

    /// 对归一化时间求曲线进度。
    pub fn sample(self, progress: f32) -> f32 {
        let x = progress.clamp(0.0, 1.0);
        match self {
            Self::Linear => x,
            Self::CubicBezier { x1, y1, x2, y2 } => {
                let x1 = x1.clamp(0.0, 1.0);
                let x2 = x2.clamp(0.0, 1.0);
                let mut low = 0.0;
                let mut high = 1.0;
                let mut parameter = x;
                for _ in 0..16 {
                    let curve_x = cubic(parameter, 0.0, x1, x2, 1.0);
                    if curve_x < x {
                        low = parameter;
                    } else {
                        high = parameter;
                    }
                    parameter = (low + high) * 0.5;
                }
                cubic(parameter, 0.0, y1, y2, 1.0).clamp(0.0, 1.0)
            }
        }
    }
}

fn cubic(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let inverse = 1.0 - t;
    inverse.powi(3) * p0
        + 3.0 * inverse.powi(2) * t * p1
        + 3.0 * inverse * t.powi(2) * p2
        + t.powi(3) * p3
}

/// 隐式 transition 的时长和曲线。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransitionSpec {
    /// 动画持续时间。
    pub duration_ms: u64,
    /// 缓动曲线。
    pub easing: Easing,
}

impl TransitionSpec {
    /// 创建 transition 声明。
    pub const fn new(duration_ms: u64, easing: Easing) -> Self {
        Self {
            duration_ms,
            easing,
        }
    }
}

/// 一个携带隐式 transition 声明的目标值。
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionTarget<T> {
    value: T,
    spec: TransitionSpec,
}

impl<T> TransitionTarget<T> {
    pub(crate) fn value(&self) -> &T {
        &self.value
    }
}

/// 为任意可插值值提供 CSS 式 `.transition(duration, easing)` 表面。
pub trait TransitionExt: Sized {
    /// 将当前值声明为隐式 transition 的目标。
    fn transition(self, duration_ms: u64, easing: Easing) -> TransitionTarget<Self> {
        TransitionTarget {
            value: self,
            spec: TransitionSpec::new(duration_ms, easing),
        }
    }
}

impl<T> TransitionExt for T {}

/// 可由 guest 单一实现插值的值。
pub trait Interpolate: Clone {
    /// 按 `0..=1` 进度插值。
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self;
}

impl Interpolate for f32 {
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self {
        from + (to - from) * progress
    }
}

impl Interpolate for Color {
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self {
        Self::rgba(
            f32::interpolate(&from.r, &to.r, progress),
            f32::interpolate(&from.g, &to.g, progress),
            f32::interpolate(&from.b, &to.b, progress),
            f32::interpolate(&from.a, &to.a, progress),
        )
    }
}

impl Interpolate for PixelOffset {
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self {
        Self {
            x: f32::interpolate(&from.x, &to.x, progress),
            y: f32::interpolate(&from.y, &to.y, progress),
        }
    }
}

impl Interpolate for BorderRadius {
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self {
        Self {
            top_left: f32::interpolate(&from.top_left, &to.top_left, progress),
            top_right: f32::interpolate(&from.top_right, &to.top_right, progress),
            bottom_right: f32::interpolate(&from.bottom_right, &to.bottom_right, progress),
            bottom_left: f32::interpolate(&from.bottom_left, &to.bottom_left, progress),
        }
    }
}

impl Interpolate for ShadowSpec {
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self {
        if from.inset != to.inset {
            return if progress < 1.0 { *from } else { *to };
        }
        Self {
            offset: PixelOffset::interpolate(&from.offset, &to.offset, progress),
            blur_radius: f32::interpolate(&from.blur_radius, &to.blur_radius, progress),
            color: Color::interpolate(&from.color, &to.color, progress),
            inset: from.inset,
        }
    }
}

impl Interpolate for Point {
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self {
        Self {
            x: f32::interpolate(&from.x, &to.x, progress),
            y: f32::interpolate(&from.y, &to.y, progress),
        }
    }
}

impl Interpolate for GradientKind {
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self {
        match (from, to) {
            (
                Self::Linear {
                    start: from_start,
                    end: from_end,
                },
                Self::Linear {
                    start: to_start,
                    end: to_end,
                },
            ) => Self::Linear {
                start: Point::interpolate(from_start, to_start, progress),
                end: Point::interpolate(from_end, to_end, progress),
            },
            (
                Self::Radial {
                    center: from_center,
                    radius: from_radius,
                },
                Self::Radial {
                    center: to_center,
                    radius: to_radius,
                },
            ) => Self::Radial {
                center: Point::interpolate(from_center, to_center, progress),
                radius: f32::interpolate(from_radius, to_radius, progress),
            },
            _ => {
                if progress < 1.0 {
                    *from
                } else {
                    *to
                }
            }
        }
    }
}

impl Interpolate for Gradient {
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self {
        if from.stops.len() != to.stops.len() {
            return if progress < 1.0 {
                from.clone()
            } else {
                to.clone()
            };
        }
        Self {
            kind: GradientKind::interpolate(&from.kind, &to.kind, progress),
            stops: from
                .stops
                .iter()
                .zip(&to.stops)
                .map(|(from, to)| ColorStop {
                    position: f32::interpolate(&from.position, &to.position, progress),
                    color: Color::interpolate(&from.color, &to.color, progress),
                })
                .collect(),
        }
    }
}

impl Interpolate for Fill {
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self {
        match (from, to) {
            (Self::Solid(from), Self::Solid(to)) => {
                Self::Solid(Color::interpolate(from, to, progress))
            }
            (Self::Linear(from), Self::Linear(to)) => {
                Self::Linear(Gradient::interpolate(from, to, progress))
            }
            (Self::Radial(from), Self::Radial(to)) => {
                Self::Radial(Gradient::interpolate(from, to, progress))
            }
            _ => {
                if progress < 1.0 {
                    from.clone()
                } else {
                    to.clone()
                }
            }
        }
    }
}

fn interpolate_option<T: Interpolate + Clone>(
    from: &Option<T>,
    to: &Option<T>,
    progress: f32,
) -> Option<T> {
    match (from, to) {
        (Some(from), Some(to)) => Some(T::interpolate(from, to, progress)),
        _ if progress < 1.0 => from.clone(),
        _ => to.clone(),
    }
}

impl Interpolate for VisualConcern {
    fn interpolate(from: &Self, to: &Self, progress: f32) -> Self {
        Self {
            fill: interpolate_option(&from.fill, &to.fill, progress),
            border_color: interpolate_option(&from.border_color, &to.border_color, progress),
            border_radius: BorderRadius::interpolate(
                &from.border_radius,
                &to.border_radius,
                progress,
            ),
            shadow: interpolate_option(&from.shadow, &to.shadow, progress),
            opacity: f32::interpolate(&from.opacity, &to.opacity, progress),
            draw_order: if progress < 1.0 {
                from.draw_order
            } else {
                to.draw_order
            },
            visual_offset: PixelOffset::interpolate(
                &from.visual_offset,
                &to.visual_offset,
                progress,
            ),
        }
    }
}

/// 一次控制器采样。
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationSample<T> {
    /// 当前插值值。
    pub value: T,
    /// 对 Host 的后续调度请求。
    pub schedule: AnimationSchedule,
}

/// 显式 ticker/controller；适合保存在组件私有跨帧 State 中。
#[derive(Clone, Debug)]
pub struct AnimationController<T> {
    from: T,
    target: T,
    current: T,
    started_at_ms: u64,
    spec: TransitionSpec,
    active: bool,
}

impl<T: Interpolate + PartialEq> AnimationController<T> {
    /// 以稳定初值创建控制器。
    pub fn new(value: T) -> Self {
        Self {
            from: value.clone(),
            target: value.clone(),
            current: value,
            started_at_ms: 0,
            spec: TransitionSpec::new(0, Easing::Linear),
            active: false,
        }
    }

    /// 解析一个 CSS 式隐式 transition 目标。
    pub fn resolve(
        &mut self,
        clock: AnimationClock,
        target: TransitionTarget<T>,
    ) -> AnimationSample<T> {
        self.advance(clock.timestamp_ms);
        if self.target != target.value {
            self.from = self.current.clone();
            self.target = target.value;
            self.started_at_ms = clock.timestamp_ms;
            self.spec = target.spec;
            self.active = self.spec.duration_ms > 0 && self.from != self.target;
            if !self.active {
                self.current = self.target.clone();
            }
        }
        self.advance(clock.timestamp_ms);
        AnimationSample {
            value: self.current.clone(),
            schedule: AnimationSchedule {
                active: self.active,
                next_deadline_ms: self.active.then(|| clock.timestamp_ms.saturating_add(16)),
            },
        }
    }

    /// 返回当前已采样值。
    pub fn value(&self) -> &T {
        &self.current
    }

    fn advance(&mut self, now_ms: u64) {
        if !self.active {
            return;
        }
        let elapsed = now_ms.saturating_sub(self.started_at_ms);
        let raw = if self.spec.duration_ms == 0 {
            1.0
        } else {
            elapsed as f32 / self.spec.duration_ms as f32
        };
        let progress = raw.clamp(0.0, 1.0);
        self.current = T::interpolate(&self.from, &self.target, self.spec.easing.sample(progress));
        if progress >= 1.0 {
            self.current = self.target.clone();
            self.active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AnimationClock, AnimationController, Easing, TransitionExt};

    #[test]
    fn injected_timestamp_sequence_is_deterministic() {
        let sample = || {
            let mut controller = AnimationController::new(0.0_f32);
            [0, 25, 50, 75, 100]
                .map(|timestamp_ms| {
                    controller
                        .resolve(
                            AnimationClock { timestamp_ms },
                            10.0.transition(100, Easing::Linear),
                        )
                        .value
                })
                .to_vec()
        };
        assert_eq!(sample(), sample());
        assert_eq!(sample(), vec![0.0, 2.5, 5.0, 7.5, 10.0]);
    }

    #[test]
    fn retarget_uses_current_sample_as_new_origin() {
        let mut controller = AnimationController::new(0.0_f32);
        controller.resolve(
            AnimationClock { timestamp_ms: 0 },
            10.0.transition(100, Easing::Linear),
        );
        assert_eq!(
            controller
                .resolve(
                    AnimationClock { timestamp_ms: 40 },
                    10.0.transition(100, Easing::Linear)
                )
                .value,
            4.0
        );
        assert_eq!(
            controller
                .resolve(
                    AnimationClock { timestamp_ms: 40 },
                    20.0.transition(100, Easing::Linear)
                )
                .value,
            4.0
        );
        assert_eq!(
            controller
                .resolve(
                    AnimationClock { timestamp_ms: 90 },
                    20.0.transition(100, Easing::Linear)
                )
                .value,
            12.0
        );
    }

    #[test]
    fn completed_animation_stops_requesting_ticks() {
        let mut controller = AnimationController::new(0.0_f32);
        controller.resolve(
            AnimationClock { timestamp_ms: 5 },
            1.0.transition(100, Easing::STANDARD),
        );
        let sample = controller.resolve(
            AnimationClock { timestamp_ms: 105 },
            1.0.transition(100, Easing::STANDARD),
        );
        assert_eq!(sample.value, 1.0);
        assert!(!sample.schedule.active);
        assert_eq!(sample.schedule.next_deadline_ms, None);
    }
}
