//! 局部快捷键：`ShortcutScope` 映射 + 冒泡 + consumed 终止（见 008-2.11）。
//!
//! - 键盘事件从当前焦点节点沿 FocusScope 冒泡；
//! - 途经 `ShortcutScope` 查表：物理组合键 → `ShortcutId`；命中即输出动作并终止冒泡；
//! - 每个 `ShortcutScope` 可局部重写键位（内层优先覆盖外层）；
//! - 跟随 FocusScope：Teleport 传送出去的子树，ShortcutScope 一同迁移（简化：按树位置冒泡）。

use tela_contract::{KeyCombo, NodeKind, RawKeyboardEvent, ShortcutId, UiNode};

/// 从焦点节点沿祖先链冒泡查快捷键。
///
/// `nodes` 为 DFS 序节点表；`parents` 为父索引表（`parents[id.0 as usize]`，
/// 由焦点图预计算，见 focus.rs）；冒泡 O(树深)，无扫描。
pub(crate) fn bubble_shortcut(
    nodes: &[&UiNode],
    focus_index: usize,
    parents: Option<&[usize]>,
    event: &RawKeyboardEvent,
) -> Option<ShortcutId> {
    let combo = KeyCombo {
        modifiers: event.modifiers,
        key: event.key,
    };
    let parents = parents?;
    let mut idx = focus_index;
    loop {
        if let Some(scope) = shortcut_scope_of(nodes[idx]) {
            for mapping in &scope.mappings {
                if mapping.combo == combo {
                    return Some(mapping.shortcut.clone());
                }
            }
        }
        let parent = parents[idx];
        if parent == idx {
            return None;
        }
        idx = parent;
    }
}

/// 节点是否带快捷键映射（ShortcutScope 逻辑容器）。
fn shortcut_scope_of(node: &UiNode) -> Option<&tela_contract::ShortcutScopeSpec> {
    match &node.kind {
        NodeKind::ShortcutScope(spec) => Some(spec),
        _ => None,
    }
}
