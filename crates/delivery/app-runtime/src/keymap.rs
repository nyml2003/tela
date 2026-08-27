//! 应用层键位表：原始平台按键 -> 语义键盘意图。
//!
//! 这个模块不依赖 DOM、WGPU 或业务 View。它持有可替换的不可变快照，并按 core 给出的
//! `KeymapScopeId` 祖先路径解析当前组合键；core 只接收最终 `KeyboardIntentEvent`。

use std::collections::{BTreeMap, HashSet};

use serde::Deserialize;
use tela_contract::{
    FocusDirection, KeyCombo, KeyState, KeyboardIntent, KeyboardIntentEvent, KeymapScopeId,
    Modifiers, PhysicalKey, RawKeyboardEvent, ShortcutId,
};

/// 键位表 JSON 协议版本。升级协议时拒绝旧/新版本，避免静默改变快捷键含义。
pub const KEYMAP_PROTOCOL_VERSION: u32 = 1;

/// 浏览器/CPU ABI 使用的修饰键位掩码：Shift。
pub const MODIFIER_SHIFT: u8 = 1 << 0;
/// 浏览器/CPU ABI 使用的修饰键位掩码：Ctrl。
pub const MODIFIER_CTRL: u8 = 1 << 1;
/// 浏览器/CPU ABI 使用的修饰键位掩码：Alt。
pub const MODIFIER_ALT: u8 = 1 << 2;
/// 浏览器/CPU ABI 使用的修饰键位掩码：Meta/Super。
pub const MODIFIER_META: u8 = 1 << 3;

/// 一条单组合键绑定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    /// 触发组合键。
    pub combo: KeyCombo,
    /// 命中后派发的语义意图。
    pub intent: KeyboardIntent,
}

/// 一层局部或默认键位表。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeymapLayer {
    /// 本层绑定列表（组合键唯一性由 [`KeymapSnapshot::validate`] 校验）。
    pub bindings: Vec<KeyBinding>,
}

/// 可替换的键位表快照。
///
/// 作用域路径必须由内向外传入 `resolve`；第一个命中的局部层覆盖外层和默认层。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeymapSnapshot {
    /// 单调递增版本；替换时禁止回退。
    pub revision: u64,
    /// 无作用域命中时使用的默认层。
    pub default_layer: KeymapLayer,
    /// 焦点作用域 -> 局部层（由内向外匹配）。
    pub scoped_layers: BTreeMap<KeymapScopeId, KeymapLayer>,
}

/// 快照校验或 JSON 传输失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeymapError {
    /// JSON 协议版本不受支持。
    UnsupportedProtocol {
        /// 收到的版本号。
        received: u32,
    },
    /// 快照版本比当前生效版本更旧。
    RevisionRegression {
        /// 当前生效版本。
        current: u64,
        /// 收到的版本。
        received: u64,
    },
    /// 同层内出现重复组合键。
    DuplicateBinding {
        /// 重复所在的层（`None` = 默认层）。
        scope: Option<KeymapScopeId>,
        /// 重复的组合键。
        combo: KeyCombo,
    },
    /// 绑定引用了不受支持的快捷键。
    UnsupportedShortcut(ShortcutId),
    /// JSON 结构或字段非法。
    InvalidWireFormat(String),
}

impl KeymapSnapshot {
    /// 基础无障碍导航键位：Tab 循环、方向键移动、Enter/Space 激活、Escape 取消、
    /// Home/End 跳到边界。等价于旧运行时内硬编码的物理键表；不含任何业务快捷键。
    pub fn navigation_default() -> Self {
        let plain = Modifiers::default();
        let shift = Modifiers {
            shift: true,
            ..Modifiers::default()
        };
        Self {
            revision: 1,
            default_layer: KeymapLayer {
                bindings: vec![
                    binding(PhysicalKey::Tab, plain, KeyboardIntent::FocusNext),
                    binding(PhysicalKey::Tab, shift, KeyboardIntent::FocusPrevious),
                    binding(
                        PhysicalKey::ArrowUp,
                        plain,
                        KeyboardIntent::MoveFocus(FocusDirection::Up),
                    ),
                    binding(
                        PhysicalKey::ArrowDown,
                        plain,
                        KeyboardIntent::MoveFocus(FocusDirection::Down),
                    ),
                    binding(
                        PhysicalKey::ArrowLeft,
                        plain,
                        KeyboardIntent::MoveFocus(FocusDirection::Left),
                    ),
                    binding(
                        PhysicalKey::ArrowRight,
                        plain,
                        KeyboardIntent::MoveFocus(FocusDirection::Right),
                    ),
                    binding(PhysicalKey::Enter, plain, KeyboardIntent::Activate),
                    binding(PhysicalKey::Space, plain, KeyboardIntent::Activate),
                    binding(PhysicalKey::Escape, plain, KeyboardIntent::Cancel),
                    binding(PhysicalKey::Home, plain, KeyboardIntent::MoveToStart),
                    binding(PhysicalKey::End, plain, KeyboardIntent::MoveToEnd),
                ],
            },
            scoped_layers: BTreeMap::new(),
        }
    }

    /// 在默认层追加一条业务组合键（构建器风格）；应用用它补齐自己的快捷键。
    pub fn with_default_binding(
        mut self,
        key: PhysicalKey,
        modifiers: Modifiers,
        intent: KeyboardIntent,
    ) -> Self {
        self.default_layer
            .bindings
            .push(binding(key, modifiers, intent));
        self
    }

    /// 解析一个按下事件。释放事件和未知组合键不消费。
    pub fn resolve(
        &self,
        raw: RawKeyboardEvent,
        scopes_inner_to_outer: &[KeymapScopeId],
    ) -> Option<KeyboardIntentEvent> {
        if raw.state != KeyState::Pressed {
            return None;
        }
        let combo = KeyCombo {
            key: raw.physical_key,
            modifiers: raw.modifiers,
        };
        let intent = scopes_inner_to_outer
            .iter()
            .filter_map(|scope| self.scoped_layers.get(scope))
            .find_map(|layer| find_binding(layer, combo))
            .or_else(|| find_binding(&self.default_layer, combo))?;
        Some(KeyboardIntentEvent {
            intent: intent.clone(),
            repeat: raw.repeat,
        })
    }

    /// 校验完整快照。`current_revision` 为当前生效快照时，禁止接收更旧版本。
    pub fn validate(&self, current_revision: Option<u64>) -> Result<(), KeymapError> {
        if let Some(current) = current_revision
            && self.revision < current
        {
            return Err(KeymapError::RevisionRegression {
                current,
                received: self.revision,
            });
        }
        validate_layer(None, &self.default_layer)?;
        for (scope, layer) in &self.scoped_layers {
            validate_layer(Some(scope.clone()), layer)?;
        }
        Ok(())
    }

    /// 解析并校验版本化 JSON。JSON 只是一种宿主传输格式，解析后仍是相同快照类型。
    pub fn from_json(json: &str) -> Result<Self, KeymapError> {
        let wire: WireSnapshot = serde_json::from_str(json)
            .map_err(|error| KeymapError::InvalidWireFormat(error.to_string()))?;
        if wire.version != KEYMAP_PROTOCOL_VERSION {
            return Err(KeymapError::UnsupportedProtocol {
                received: wire.version,
            });
        }
        let default_layer = KeymapLayer {
            bindings: wire
                .default_layer
                .into_iter()
                .map(WireBinding::into_binding)
                .collect::<Result<_, _>>()?,
        };
        let scoped_layers = wire
            .scoped_layers
            .into_iter()
            .map(|(id, bindings)| {
                let bindings = bindings
                    .into_iter()
                    .map(WireBinding::into_binding)
                    .collect::<Result<_, KeymapError>>()?;
                Ok((KeymapScopeId(id), KeymapLayer { bindings }))
            })
            .collect::<Result<_, KeymapError>>()?;
        let snapshot = Self {
            revision: wire.revision,
            default_layer,
            scoped_layers,
        };
        snapshot.validate(None)?;
        Ok(snapshot)
    }
}

/// 从稳定 ABI 参数创建原始按键事件。未知码不进入 UI 输入链。
pub fn raw_key_from_codes(code: u16, modifier_bits: u8, repeat: bool) -> Option<RawKeyboardEvent> {
    Some(RawKeyboardEvent {
        physical_key: PhysicalKey::from_code(code)?,
        modifiers: Modifiers {
            shift: modifier_bits & MODIFIER_SHIFT != 0,
            ctrl: modifier_bits & MODIFIER_CTRL != 0,
            alt: modifier_bits & MODIFIER_ALT != 0,
            meta: modifier_bits & MODIFIER_META != 0,
        },
        state: KeyState::Pressed,
        repeat,
    })
}

fn binding(key: PhysicalKey, modifiers: Modifiers, intent: KeyboardIntent) -> KeyBinding {
    KeyBinding {
        combo: KeyCombo { key, modifiers },
        intent,
    }
}

fn find_binding(layer: &KeymapLayer, combo: KeyCombo) -> Option<&KeyboardIntent> {
    layer
        .bindings
        .iter()
        .find(|binding| binding.combo == combo)
        .map(|binding| &binding.intent)
}

fn validate_layer(scope: Option<KeymapScopeId>, layer: &KeymapLayer) -> Result<(), KeymapError> {
    let mut seen = HashSet::with_capacity(layer.bindings.len());
    for binding in &layer.bindings {
        if !seen.insert(binding.combo) {
            return Err(KeymapError::DuplicateBinding {
                scope,
                combo: binding.combo,
            });
        }
        if let KeyboardIntent::Invoke(shortcut) = &binding.intent
            && !is_supported_shortcut(shortcut)
        {
            return Err(KeymapError::UnsupportedShortcut(shortcut.clone()));
        }
    }
    Ok(())
}

fn is_supported_shortcut(shortcut: &ShortcutId) -> bool {
    matches!(shortcut, ShortcutId::Undo)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSnapshot {
    version: u32,
    revision: u64,
    #[serde(default)]
    default_layer: Vec<WireBinding>,
    #[serde(default)]
    scoped_layers: BTreeMap<String, Vec<WireBinding>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireBinding {
    key: String,
    #[serde(default)]
    modifiers: Vec<String>,
    intent: WireIntent,
}

impl WireBinding {
    fn into_binding(self) -> Result<KeyBinding, KeymapError> {
        Ok(KeyBinding {
            combo: KeyCombo {
                key: parse_key(&self.key)?,
                modifiers: parse_modifiers(&self.modifiers)?,
            },
            intent: self.intent.into_intent()?,
        })
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum WireIntent {
    FocusNext,
    FocusPrevious,
    MoveFocus { direction: String },
    Activate,
    Cancel,
    Invoke { shortcut: String },
}

impl WireIntent {
    fn into_intent(self) -> Result<KeyboardIntent, KeymapError> {
        let invalid = |value: &str| KeymapError::InvalidWireFormat(value.to_owned());
        match self {
            Self::FocusNext => Ok(KeyboardIntent::FocusNext),
            Self::FocusPrevious => Ok(KeyboardIntent::FocusPrevious),
            Self::MoveFocus { direction } => {
                Ok(KeyboardIntent::MoveFocus(match direction.as_str() {
                    "up" => FocusDirection::Up,
                    "down" => FocusDirection::Down,
                    "left" => FocusDirection::Left,
                    "right" => FocusDirection::Right,
                    _ => return Err(invalid(&format!("unknown focus direction: {direction}"))),
                }))
            }
            Self::Activate => Ok(KeyboardIntent::Activate),
            Self::Cancel => Ok(KeyboardIntent::Cancel),
            Self::Invoke { shortcut } => Ok(KeyboardIntent::Invoke(match shortcut.as_str() {
                "undo" => ShortcutId::Undo,
                _ => return Err(invalid(&format!("unsupported shortcut: {shortcut}"))),
            })),
        }
    }
}

fn parse_key(value: &str) -> Result<PhysicalKey, KeymapError> {
    let code = match value {
        "KeyA" => 0x04,
        "KeyB" => 0x05,
        "KeyC" => 0x06,
        "KeyD" => 0x07,
        "KeyE" => 0x08,
        "KeyF" => 0x09,
        "KeyG" => 0x0a,
        "KeyH" => 0x0b,
        "KeyI" => 0x0c,
        "KeyJ" => 0x0d,
        "KeyK" => 0x0e,
        "KeyL" => 0x0f,
        "KeyM" => 0x10,
        "KeyN" => 0x11,
        "KeyO" => 0x12,
        "KeyP" => 0x13,
        "KeyQ" => 0x14,
        "KeyR" => 0x15,
        "KeyS" => 0x16,
        "KeyT" => 0x17,
        "KeyU" => 0x18,
        "KeyV" => 0x19,
        "KeyW" => 0x1a,
        "KeyX" => 0x1b,
        "KeyY" => 0x1c,
        "KeyZ" => 0x1d,
        "Digit1" => 0x1e,
        "Digit2" => 0x1f,
        "Digit3" => 0x20,
        "Digit4" => 0x21,
        "Digit5" => 0x22,
        "Digit6" => 0x23,
        "Digit7" => 0x24,
        "Digit8" => 0x25,
        "Digit9" => 0x26,
        "Digit0" => 0x27,
        "Enter" => 0x28,
        "Escape" => 0x29,
        "Backspace" => 0x2a,
        "Tab" => 0x2b,
        "Space" => 0x2c,
        "Insert" => 0x49,
        "Home" => 0x4a,
        "PageUp" => 0x4b,
        "Delete" => 0x4c,
        "End" => 0x4d,
        "PageDown" => 0x4e,
        "ArrowRight" => 0x4f,
        "ArrowLeft" => 0x50,
        "ArrowDown" => 0x51,
        "ArrowUp" => 0x52,
        _ => {
            return Err(KeymapError::InvalidWireFormat(format!(
                "unknown physical key: {value}"
            )));
        }
    };
    PhysicalKey::from_code(code)
        .ok_or_else(|| KeymapError::InvalidWireFormat(format!("invalid physical key: {value}")))
}

fn parse_modifiers(values: &[String]) -> Result<Modifiers, KeymapError> {
    let mut modifiers = Modifiers::default();
    for value in values {
        let slot = match value.as_str() {
            "shift" => &mut modifiers.shift,
            "ctrl" => &mut modifiers.ctrl,
            "alt" => &mut modifiers.alt,
            "meta" => &mut modifiers.meta,
            _ => {
                return Err(KeymapError::InvalidWireFormat(format!(
                    "unknown modifier: {value}"
                )));
            }
        };
        if *slot {
            return Err(KeymapError::InvalidWireFormat(format!(
                "duplicate modifier: {value}"
            )));
        }
        *slot = true;
    }
    Ok(modifiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(key: PhysicalKey, modifiers: Modifiers) -> RawKeyboardEvent {
        RawKeyboardEvent {
            physical_key: key,
            modifiers,
            state: KeyState::Pressed,
            repeat: false,
        }
    }

    #[test]
    fn navigation_default_keeps_the_legacy_navigation_table() {
        let keymap = KeymapSnapshot::navigation_default();
        assert_eq!(
            keymap
                .resolve(raw(PhysicalKey::Tab, Modifiers::default()), &[])
                .expect("Tab 应命中")
                .intent,
            KeyboardIntent::FocusNext
        );
        assert!(
            keymap
                .resolve(
                    raw(
                        PhysicalKey::KeyZ,
                        Modifiers {
                            ctrl: true,
                            ..Modifiers::default()
                        }
                    ),
                    &[]
                )
                .is_none(),
            "基础导航表不应包含业务快捷键"
        );
    }

    #[test]
    fn local_scope_overrides_default_layer() {
        let scope = KeymapScopeId("modal".to_owned());
        let mut keymap = KeymapSnapshot::navigation_default().with_default_binding(
            PhysicalKey::KeyZ,
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            KeyboardIntent::Invoke(ShortcutId::Undo),
        );
        keymap.scoped_layers.insert(
            scope.clone(),
            KeymapLayer {
                bindings: vec![binding(
                    PhysicalKey::KeyZ,
                    Modifiers::default(),
                    KeyboardIntent::Cancel,
                )],
            },
        );
        let resolved = keymap
            .resolve(raw(PhysicalKey::KeyZ, Modifiers::default()), &[scope])
            .expect("作用域绑定应命中");
        assert_eq!(resolved.intent, KeyboardIntent::Cancel);
    }

    #[test]
    fn rejects_duplicate_combo_and_revision_regression() {
        let combo = binding(
            PhysicalKey::Tab,
            Modifiers::default(),
            KeyboardIntent::FocusNext,
        );
        let duplicate = KeymapSnapshot {
            revision: 1,
            default_layer: KeymapLayer {
                bindings: vec![combo.clone(), combo],
            },
            scoped_layers: BTreeMap::new(),
        };
        assert!(matches!(
            duplicate.validate(None),
            Err(KeymapError::DuplicateBinding { .. })
        ));
        let older = KeymapSnapshot {
            revision: 0,
            ..KeymapSnapshot::navigation_default()
        };
        assert!(matches!(
            older.validate(Some(1)),
            Err(KeymapError::RevisionRegression { .. })
        ));
    }

    #[test]
    fn parses_versioned_json_into_the_same_snapshot_model() {
        let snapshot = KeymapSnapshot::from_json(
            r#"{
                "version": 1,
                "revision": 4,
                "default_layer": [
                    {"key":"Tab","intent":{"type":"focus_next"}}
                ],
                "scoped_layers": {
                    "dialog": [
                        {"key":"Escape","intent":{"type":"cancel"}}
                    ]
                }
            }"#,
        )
        .expect("合法 JSON 快照");
        assert_eq!(snapshot.revision, 4);
        assert!(
            snapshot
                .scoped_layers
                .contains_key(&KeymapScopeId("dialog".to_owned()))
        );
    }
}
