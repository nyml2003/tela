//! 通用数值滑块：受控最终值、范围约束与线性/对数刻度。

use tela_contract::{
    BindId, Color, Fill, IdentityConcern, InteractConcern, KeyStrategy, LayoutConcern, PixelOffset,
    SemanticKey, Size, UiNode, UpdateMode, VisualConcern,
};
use tela_core::{LayoutContainer, Primitive};

/// 滑块的刻度方式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SliderScale {
    /// 普通线性刻度。
    #[default]
    Linear,
    /// 正值范围的对数刻度，适合以 1 为中心的倍率控制。
    Logarithmic,
}

/// 滑块的受控配置。
#[derive(Clone, Debug, PartialEq)]
pub struct SliderConfig {
    /// 最小值。
    pub min: f64,
    /// 最大值。
    pub max: f64,
    /// 当前已确认值。
    pub value: f64,
    /// 键盘和离散拖动步进；小于等于零时按连续值处理。
    pub step: Option<f64>,
    /// 刻度方式。
    pub scale: SliderScale,
}

impl Default for SliderConfig {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 100.0,
            value: 0.0,
            step: None,
            scale: SliderScale::Linear,
        }
    }
}

impl SliderConfig {
    /// 将值限制在范围内，并按可选步进吸附。
    pub fn normalize(&self, value: f64) -> f64 {
        let min = self.min.min(self.max);
        let max = self.min.max(self.max);
        let mut value = value.clamp(min, max);
        if let Some(step) = self.step.filter(|step| *step > 0.0) {
            value = (min + ((value - min) / step).round() * step).clamp(min, max);
        }
        value
    }

    /// 将值投影到 0..1 的轨道位置。
    pub fn position(&self, value: f64) -> f64 {
        let min = self.min.min(self.max);
        let max = self.min.max(self.max);
        if (max - min).abs() < f64::EPSILON {
            return 0.0;
        }
        match self.scale {
            SliderScale::Linear => ((self.normalize(value) - min) / (max - min)).clamp(0.0, 1.0),
            SliderScale::Logarithmic if min > 0.0 && max > 0.0 => {
                let value = self.normalize(value).max(f64::MIN_POSITIVE);
                ((value.ln() - min.ln()) / (max.ln() - min.ln())).clamp(0.0, 1.0)
            }
            SliderScale::Logarithmic => {
                ((self.normalize(value) - min) / (max - min)).clamp(0.0, 1.0)
            }
        }
    }

    /// 将 0..1 的轨道位置转换为已规范化值。
    pub fn value_at(&self, position: f64) -> f64 {
        let min = self.min.min(self.max);
        let max = self.min.max(self.max);
        let position = position.clamp(0.0, 1.0);
        let value = match self.scale {
            SliderScale::Linear => min + (max - min) * position,
            SliderScale::Logarithmic if min > 0.0 && max > 0.0 => {
                (min.ln() + (max.ln() - min.ln()) * position).exp()
            }
            SliderScale::Logarithmic => min + (max - min) * position,
        };
        self.normalize(value)
    }
}

/// 滑块内部交互事件。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SliderEvent {
    /// 指针在轨道上的归一化位置。
    Position(f64),
    /// 键盘向前一步。
    Increment,
    /// 键盘向后一步。
    Decrement,
    /// 跳到最小值。
    Home,
    /// 跳到最大值。
    End,
}

/// 一次滑块事件的受控输出。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderOutcome {
    /// 规范化后的目标值。
    pub value: f64,
    /// 是否相对旧值发生变化。
    pub changed: bool,
}

/// 通用滑块节点。
pub struct Slider {
    config: SliderConfig,
    width: f32,
    disabled: bool,
    bind_id: Option<BindId>,
    semantic_key: Option<SemanticKey>,
    track: Color,
    active: Color,
    thumb: Color,
}

impl Slider {
    /// 创建一个默认滑块。
    pub fn new(config: SliderConfig) -> Self {
        Self {
            config: SliderConfig {
                value: config.normalize(config.value),
                ..config
            },
            width: 240.0,
            disabled: false,
            bind_id: None,
            semantic_key: None,
            track: Color::rgba(0.84, 0.87, 0.92, 1.0),
            active: Color::rgba(0.10, 0.40, 0.85, 1.0),
            thumb: Color::WHITE,
        }
    }

    /// 设置轨道宽度。
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(40.0);
        self
    }

    /// 设置禁用态。
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// 设置业务值绑定；它不参与组件身份。
    pub fn bind_id(mut self, bind_id: impl Into<String>) -> Self {
        self.bind_id = Some(BindId(bind_id.into()));
        self
    }

    /// 设置稳定语义 key。
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.semantic_key = Some(SemanticKey(key.into()));
        self
    }

    /// 读取当前配置。
    pub fn config(&self) -> &SliderConfig {
        &self.config
    }

    /// 处理一个局部滑块事件，返回应交给上层确认的值。
    pub fn handle(&self, event: SliderEvent) -> SliderOutcome {
        let step = self
            .config
            .step
            .filter(|step| *step > 0.0)
            .unwrap_or_else(|| (self.config.max - self.config.min).abs() / 100.0);
        let raw = match event {
            SliderEvent::Position(position) => self.config.value_at(position),
            SliderEvent::Increment => self.config.value + step,
            SliderEvent::Decrement => self.config.value - step,
            SliderEvent::Home => self.config.min,
            SliderEvent::End => self.config.max,
        };
        let value = self.config.normalize(raw);
        SliderOutcome {
            value,
            changed: (value - self.config.value).abs() > f64::EPSILON,
        }
    }

    /// 生成固定尺寸的滑块轨道与拇指。
    pub fn into_node(self) -> UiNode {
        let position = self.config.position(self.config.value) as f32;
        let track: UiNode = Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(4.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(self.track)),
                border_radius: tela_contract::BorderRadius::all(2.0),
                ..VisualConcern::default()
            })
            .into();
        let active: UiNode = Primitive::rect()
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width * position)),
                height: Some(Size::fixed(4.0)),
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(if self.disabled {
                    self.track
                } else {
                    self.active
                })),
                border_radius: tela_contract::BorderRadius::all(2.0),
                ..VisualConcern::default()
            })
            .into();
        let thumb: UiNode = Primitive::circle()
            .layout(LayoutConcern {
                width: Some(Size::fixed(14.0)),
                height: Some(Size::fixed(14.0)),
                border_width: 2.0,
                ..LayoutConcern::default()
            })
            .visual(VisualConcern {
                fill: Some(Fill::Solid(if self.disabled {
                    self.track
                } else {
                    self.thumb
                })),
                border_color: Some(if self.disabled {
                    self.track
                } else {
                    self.active
                }),
                visual_offset: PixelOffset {
                    x: self.width * position - 7.0,
                    y: 3.0,
                },
                ..VisualConcern::default()
            })
            .into();
        let mut node: UiNode = LayoutContainer::stack([track, active, thumb])
            .layout(LayoutConcern {
                width: Some(Size::fixed(self.width)),
                height: Some(Size::fixed(20.0)),
                ..LayoutConcern::default()
            })
            .into();
        if let Some(key) = self.semantic_key {
            node.identity = Some(IdentityConcern {
                key_strategy: KeyStrategy::SemanticId,
                semantic_key: Some(key),
                update_mode: UpdateMode::Dirty,
                ..IdentityConcern::default()
            });
        }
        if !self.disabled {
            node.interact = Some(InteractConcern {
                clickable: true,
                hoverable: true,
                focusable: true,
                bind_id: self.bind_id,
                ..InteractConcern::default()
            });
        }
        node
    }
}

impl From<Slider> for UiNode {
    fn from(value: Slider) -> Self {
        value.into_node()
    }
}

#[cfg(test)]
mod tests {
    use super::{SliderConfig, SliderEvent, SliderScale};

    #[test]
    fn logarithmic_scale_places_normal_speed_at_the_center() {
        let config = SliderConfig {
            min: 0.25,
            max: 4.0,
            value: 1.0,
            step: None,
            scale: SliderScale::Logarithmic,
        };
        assert!((config.position(1.0) - 0.5).abs() < 1e-9);
        assert!((config.value_at(0.5) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn step_and_range_are_applied_to_keyboard_events() {
        let config = SliderConfig {
            min: 0.25,
            max: 4.0,
            value: 1.0,
            step: Some(0.25),
            scale: SliderScale::Logarithmic,
        };
        let slider = super::Slider::new(config);
        assert_eq!(slider.handle(SliderEvent::Increment).value, 1.25);
        assert_eq!(slider.handle(SliderEvent::Home).value, 0.25);
    }
}
