//! 稳定身份分配：`auto-stable-identity` 策略（见 005-key身份策略 2.2）。
//!
//! 容器声明 `KeyStrategy::AutoStableIdentity` 后，作用域内节点首次出现由分配器分配
//! 内部稳定身份；后续帧按三条规则分配：
//! 1. 同相对路径 + 内容指纹相同 → 优先复用（位置优先）；
//! 2. 否则在闲置池中找指纹相同且未回收的 id（支持列表增删/重排）；
//! 3. 否则分配全新 id；id 延迟 N 帧回收；scope 之间隔离。
//!
//! 实现采用"无常驻指纹索引"路线：闲置 id 集合天然很小（仅删除后未老化回收的节点），
//! 直接线性扫描；闲置 id 数量超阈值（64）时临时构造一次性指纹映射，用完即弃。
//! 维护的不变量：`by_path` 永远是上帧快照（`end_frame` 经 `take` 轮转）；`id_to_fp`
//! 随 id 存活/回收同步增删；`used_this_frame` 帧末清空；scope 闲置超阈值整体回收。

use std::collections::{HashMap, HashSet};
use tela_contract::{ContentConcern, KeyStrategy, SemanticKey, UiNode};

/// 稳定身份分配器（宿主跨帧传入 `UiTree::new_with_allocator`）。
#[derive(Default)]
pub struct IdentityAllocator {
    /// 作用域容器 key → 分配表。
    tables: HashMap<SemanticKey, StableTable>,
    /// 延迟回收帧数上限（id 与 scope 共用）。
    max_unused_frames: u32,
}

/// 单作用域分配表。
#[derive(Default)]
struct StableTable {
    next_id: u64,
    /// 上帧快照：相对路径 → id（位置优先匹配；每帧由 `path_this_frame` 轮转，无脏累积）。
    by_path: HashMap<String, u64>,
    /// 本帧分配结果，`end_frame` 接管成为下一轮 `by_path`。
    path_this_frame: HashMap<String, u64>,
    /// id → 内容指纹（唯一反查表，id 回收时同步删除）。
    id_to_fp: HashMap<u64, u64>,
    /// 本帧已占用的 id，防止同一 scope 同一帧重复使用。
    used_this_frame: HashSet<u64>,
    /// id → 连续闲置帧数；只含尚未回收的存活 id（活跃 id 年龄为 0）。
    unused_frames: HashMap<u64, u32>,
    /// 本帧是否被访问（scope 整体回收依据）。
    touched_this_frame: bool,
    /// scope 连续未出现帧数。
    scope_unused_frames: u32,
}

/// 闲置池线性扫描转临时指纹映射的阈值。
const IDLE_FINGERPRINT_THRESHOLD: usize = 64;

impl IdentityAllocator {
    /// 新建分配器（默认延迟回收 8 帧）。
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
            max_unused_frames: 8,
        }
    }

    /// 设置延迟回收帧数上限（id 与 scope 共用）。
    pub fn set_max_unused_frames(&mut self, frames: u32) {
        self.max_unused_frames = frames;
    }

    /// 为作用域内一个节点分配稳定身份，返回稳定 key。
    ///
    /// `scope_key` 为声明 `AutoStableIdentity` 的容器 key；`path` 为作用域内相对路径。
    pub(crate) fn assign(
        &mut self,
        scope_key: &SemanticKey,
        path: &str,
        node: &UiNode,
    ) -> SemanticKey {
        let table = self.tables.entry(scope_key.clone()).or_default();
        let id = table.assign(path, fingerprint(node));
        SemanticKey(format!("stable:{}:{id}", scope_key.0))
    }

    /// 标记作用域容器本帧存在（即使无子节点分配，也保活该表，见 `end_frame` 回收语义）。
    pub(crate) fn touch(&mut self, scope_key: &SemanticKey) {
        self.tables
            .entry(scope_key.clone())
            .or_default()
            .touched_this_frame = true;
    }

    /// 帧末：id 老化回收 + scope 整体回收（由 `UiTree::new_with_allocator` 调用）。
    pub(crate) fn end_frame(&mut self) {
        let max_scope_unused = self.max_unused_frames;
        let mut remove_scopes: Vec<SemanticKey> = Vec::new();
        for (scope_key, table) in self.tables.iter_mut() {
            table.end_frame(max_scope_unused);
            if table.touched_this_frame {
                table.scope_unused_frames = 0;
            } else {
                table.scope_unused_frames += 1;
                if table.scope_unused_frames >= max_scope_unused {
                    remove_scopes.push(scope_key.clone());
                }
            }
            table.touched_this_frame = false;
        }
        for key in remove_scopes {
            self.tables.remove(&key);
        }
    }

    /// 调试/测试：当前存活的分配表数量。
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }
}

impl StableTable {
    fn assign(&mut self, path: &str, fp: u64) -> u64 {
        self.touched_this_frame = true;

        // 1. 位置优先匹配：上帧同路径，指纹相同，且未被本帧占用。
        if let Some(&id) = self.by_path.get(path)
            && !self.used_this_frame.contains(&id)
            && self.id_to_fp.get(&id) == Some(&fp)
        {
            self.mark_used(path, id);
            return id;
        }

        // 2. 闲置池按指纹复用：当前真正空闲的 id（存活且本帧未占用）。
        let idle_ids: Vec<u64> = self
            .unused_frames
            .keys()
            .filter(|id| !self.used_this_frame.contains(id))
            .copied()
            .collect();
        let reuse_id = if idle_ids.len() > IDLE_FINGERPRINT_THRESHOLD {
            // 闲置很多：临时构造一次性指纹映射，用完即弃，不保存。
            let mut fp_map: HashMap<u64, Vec<u64>> = HashMap::new();
            for &id in &idle_ids {
                if let Some(f) = self.id_to_fp.get(&id) {
                    fp_map.entry(*f).or_default().push(id);
                }
            }
            fp_map.get(&fp).and_then(|ids| ids.first()).copied()
        } else {
            idle_ids
                .iter()
                .find(|id| self.id_to_fp.get(id) == Some(&fp))
                .copied()
        };
        if let Some(id) = reuse_id {
            self.mark_used(path, id);
            return id;
        }

        // 3. 全新身份。
        let id = self.next_id;
        self.next_id += 1;
        self.id_to_fp.insert(id, fp);
        self.mark_used(path, id);
        id
    }

    fn mark_used(&mut self, path: &str, id: u64) {
        self.path_this_frame.insert(path.to_string(), id);
        self.used_this_frame.insert(id);
        // 本帧重新启用：闲置年龄清零。
        *self.unused_frames.entry(id).or_insert(0) = 0;
    }

    fn end_frame(&mut self, max_unused: u32) {
        // 1. 年龄更新：本帧使用的清零，未使用的 +1，连续闲置达到上限即回收。
        let mut to_remove: Vec<u64> = Vec::new();
        for (&id, age) in &mut self.unused_frames {
            if self.used_this_frame.contains(&id) {
                *age = 0;
            } else {
                *age += 1;
                if *age >= max_unused {
                    to_remove.push(id);
                }
            }
        }
        // 2. 回收过期 id（无二级索引要清理）。
        for id in to_remove {
            self.unused_frames.remove(&id);
            self.id_to_fp.remove(&id);
        }
        // 3. 快照轮转：本帧路径映射成为下一帧的上帧快照。
        self.by_path = std::mem::take(&mut self.path_this_frame);
        // 4. 本帧状态清零。
        self.used_this_frame.clear();
    }
}

/// 内容指纹：类型 + 内容关键属性（类型变化 → 新身份，见 005-2.2）。
///
/// 只使用 kind 与内容（文本）作为身份指纹——尺寸/视觉变化不改变身份（状态随内容保持）。
/// 采用确定性 FNV-1a（`DefaultHasher` 随机种子会导致跨帧指纹不稳定，破坏身份匹配）。
fn fingerprint(node: &UiNode) -> u64 {
    let mut h = FnvHasher::new();
    match &node.kind {
        tela_contract::NodeKind::Text => h.write(1),
        tela_contract::NodeKind::Image => h.write(2),
        tela_contract::NodeKind::Rect => h.write(3),
        tela_contract::NodeKind::Circle => h.write(4),
        tela_contract::NodeKind::Ellipse => h.write(5),
        tela_contract::NodeKind::NinePatch => h.write(6),
        tela_contract::NodeKind::Polygon => h.write(7),
        _ => h.write(0),
    }
    match &node.content {
        Some(ContentConcern::Text(text)) => {
            h.write(1);
            h.write_str(&text.text);
        }
        Some(ContentConcern::Image(image)) => {
            h.write(2);
            h.write_str(&image.texture.0);
        }
        Some(ContentConcern::NinePatch(nine)) => {
            h.write(3);
            h.write_str(&nine.texture.0);
        }
        Some(ContentConcern::Polygon { points }) => {
            h.write(4);
            h.write_u64(points.len() as u64);
        }
        Some(ContentConcern::Empty) | None => h.write(0),
    }
    h.finish()
}

/// 确定性 FNV-1a 哈希（跨帧稳定，见 005-2.2 内容指纹）。
pub(crate) struct FnvHasher(u64);

impl FnvHasher {
    pub(crate) fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    pub(crate) fn write(&mut self, byte: u8) {
        self.0 ^= byte as u64;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }
    pub(crate) fn write_u64(&mut self, value: u64) {
        for i in 0..8 {
            self.write(((value >> (i * 8)) & 0xff) as u8);
        }
    }
    pub(crate) fn write_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.write(b);
        }
    }
    pub(crate) fn finish(&self) -> u64 {
        self.0
    }
}

/// 节点是否为 auto-stable 作用域容器（身份策略向下生效，子容器可覆盖）。
pub(crate) fn is_stable_scope(node: &UiNode) -> bool {
    node.identity
        .as_ref()
        .is_some_and(|i| i.key_strategy == KeyStrategy::AutoStableIdentity)
}
