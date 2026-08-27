//! 中继共享状态：事件日志、agent 连接枢纽、权限状态机与请求去重。
//!
//! 全部字段经 `Mutex`/原子量串行化；线程模型是"每连接一线程 + 一条清扫线程"，无异步
//! 运行时。`RelayState` 以 `Arc` 共享给 HTTP、agent 链路与清扫线程。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tela_cc_protocol::{
    DownlinkMessage, EVENT_LOG_CAP, Event, EventKind, MAX_EVENT_BYTES, PermissionDecision,
    PermissionResolver,
};

use crate::persist::PersistHandle;

/// UTC 毫秒时间戳（1970 起）；时钟回拨时取 0，由 seq 保证顺序语义。
pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 中继内部错误（事件过大、agent 掉线等）；HTTP 层映射为状态码 + `ErrorBody`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayError(pub String);

impl std::fmt::Display for RelayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RelayError {}

/// 追加事件失败的已见原因只有体积超限；用小函数保持调用点可读。
fn oversize(kind: &EventKind) -> RelayError {
    let bytes = serde_json::to_vec(kind).map_or(0, |v| v.len());
    RelayError(format!(
        "event of {bytes} bytes exceeds MAX_EVENT_BYTES={MAX_EVENT_BYTES}"
    ))
}

// ---------------------------------------------------------------------------
// 事件日志。
// ---------------------------------------------------------------------------

/// 每用户全局单调的事件时间线：`seq` 无空洞，内存按 [`EVENT_LOG_CAP`] 环形裁剪。
pub struct EventLog {
    events: Mutex<VecDeque<Event>>,
    next_seq: AtomicU64,
}

impl EventLog {
    /// 空日志；`seq` 从 1 开始。
    pub fn empty() -> Self {
        Self {
            events: Mutex::new(VecDeque::new()),
            next_seq: AtomicU64::new(1),
        }
    }

    /// 从持久层重放构建；`seq` 从现有最大值续增，不回退。
    pub fn from_replay(events: Vec<Event>) -> Self {
        let mut restored = VecDeque::with_capacity(EVENT_LOG_CAP.min(events.len().max(1)));
        let mut latest = 0;
        for event in events {
            latest = latest.max(event.seq);
            if restored.len() == EVENT_LOG_CAP {
                restored.pop_front();
            }
            restored.push_back(event);
        }
        Self {
            events: Mutex::new(restored),
            next_seq: AtomicU64::new(latest + 1),
        }
    }

    /// 追加一条事件：赋 `seq`/`ts_ms`、入队、环形裁剪并按需持久化。
    pub fn append(&self, kind: EventKind) -> Result<Event, RelayError> {
        if serde_json::to_vec(&kind).map_or(true, |v| v.len() > MAX_EVENT_BYTES) {
            return Err(oversize(&kind));
        }
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let event = Event {
            seq,
            ts_ms: now_ms(),
            kind,
        };
        let mut events = self.events.lock().expect("event log poisoned");
        if events.len() == EVENT_LOG_CAP {
            events.pop_front();
        }
        events.push_back(event.clone());
        Ok(event)
    }

    /// 已分配的最大 `seq`（无事件时为 0）；重放后保持语义。
    pub fn latest_seq(&self) -> u64 {
        self.next_seq.load(Ordering::SeqCst).saturating_sub(1)
    }

    /// 下一条将被分配的 `seq`（供手机消息预生成 `turn_id`）。
    pub fn peek_next_seq(&self) -> u64 {
        self.next_seq.load(Ordering::SeqCst)
    }

    /// 读取 `seq > since` 的最多 `limit` 条事件；返回 (事件, 是否还有更多)。
    ///
    /// 环形裁剪会让早期事件消失：`since` 落到 floor 之前时静默钳到 floor（抬高游标，
    /// 返回现存最早的事件）——v1 取舍是不在这里报 `cursor_reset`（那只在整条日志重启
    /// 归零时发生，由 HTTP 层用 latest_seq 判定）。
    pub fn since(&self, since: u64, limit: usize) -> (Vec<Event>, bool) {
        let events = self.events.lock().expect("event log poisoned");
        let floor = events
            .front()
            .map_or(u64::MAX, |event| event.seq.saturating_sub(1));
        let effective = since.max(floor);
        let mut page = Vec::new();
        let mut more = false;
        for event in events.iter().skip_while(|event| event.seq <= effective) {
            if page.len() == limit {
                more = true;
                break;
            }
            page.push(event.clone());
        }
        (page, more)
    }
}

// ---------------------------------------------------------------------------
// agent 连接枢纽。
// ---------------------------------------------------------------------------

/// 当前 agent 连接：写端通道 + 连接 id。新连接替换旧连接（v1 单 agent）。
struct AgentConnection {
    id: u64,
    sender: Sender<DownlinkMessage>,
}

/// agent 在线状态与命令下发通道。
pub struct AgentHub {
    connection: Mutex<Option<AgentConnection>>,
    next_id: AtomicU64,
}

impl AgentHub {
    /// 无连接的枢纽。
    pub fn empty() -> Self {
        Self {
            connection: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }

    /// 是否有 agent 在线。
    pub fn online(&self) -> bool {
        self.connection
            .lock()
            .expect("agent hub poisoned")
            .is_some()
    }

    /// 附着新连接，返回连接 id；被替换的旧连接收到 Bye 帧后由其写线程退出。
    pub fn attach(&self, agent_id: String, sender: Sender<DownlinkMessage>) -> u64 {
        let mut connection = self.connection.lock().expect("agent hub poisoned");
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let _ = agent_id;
        if let Some(old) = connection.replace(AgentConnection { id, sender }) {
            let _ = old.sender.send(DownlinkMessage::Error {
                text: "replaced by a newer agent connection".to_owned(),
            });
        }
        id
    }

    /// 仅当 `connection_id` 仍是当前连接时摘除（防止旧连接线程清掉新连接）。
    pub fn detach(&self, connection_id: u64) {
        let mut connection = self.connection.lock().expect("agent hub poisoned");
        if connection
            .as_ref()
            .is_some_and(|current| current.id == connection_id)
        {
            *connection = None;
        }
    }

    /// 下发一条命令；掉线时报错。
    pub fn send(&self, message: DownlinkMessage) -> Result<(), RelayError> {
        let connection = self.connection.lock().expect("agent hub poisoned");
        connection
            .as_ref()
            .ok_or_else(|| RelayError("agent offline".to_owned()))
            .and_then(|current| {
                current.sender.send(message).map_err(|_| {
                    RelayError("agent went offline while sending a command".to_owned())
                })
            })
    }
}

// ---------------------------------------------------------------------------
// 权限状态机。
// ---------------------------------------------------------------------------

/// 一次权限请求的登记与裁决。
struct PermissionEntry {
    expires_at_ms: u64,
    resolution: Option<(PermissionDecision, PermissionResolver)>,
}

/// `decide` 的结果：新裁决或已被他方裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecideOutcome {
    /// 本次调用完成裁决。
    Resolved(PermissionDecision, PermissionResolver),
    /// 已有裁决（后到者；HTTP 409）。
    AlreadyResolved(PermissionDecision, PermissionResolver),
}

/// 挂起权限表：`Pending → Resolved` 单向；清扫线程把过期未决项判为 relay-expired。
pub struct PermissionTable {
    entries: Mutex<HashMap<String, PermissionEntry>>,
}

impl PermissionTable {
    /// 空表。
    pub fn empty() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// 登记一条挂起权限（agent 上行 `permission_requested` 时调用；重复登记刷新有效期）。
    pub fn register(&self, permission_id: String, expires_at_ms: u64) {
        self.entries
            .lock()
            .expect("permission table poisoned")
            .insert(
                permission_id,
                PermissionEntry {
                    expires_at_ms,
                    resolution: None,
                },
            );
    }

    /// 手机侧裁决；单向状态机。
    pub fn decide(
        &self,
        permission_id: &str,
        decision: PermissionDecision,
        now_ms: u64,
    ) -> Result<DecideOutcome, RelayError> {
        let mut entries = self.entries.lock().expect("permission table poisoned");
        let entry = entries
            .get_mut(permission_id)
            .ok_or_else(|| RelayError(format!("unknown permission {permission_id}")))?;
        if let Some((existing, resolver)) = entry.resolution {
            return Ok(DecideOutcome::AlreadyResolved(existing, resolver));
        }
        // 手机通常在有效期内答复；过清扫宽限仍未决的权项已由清扫线程收口，这里不再受理。
        if now_ms > entry.expires_at_ms + SWEEP_GRACE_MS {
            return Err(RelayError(format!(
                "permission {permission_id} already expired"
            )));
        }
        entry.resolution = Some((decision, PermissionResolver::Phone));
        Ok(DecideOutcome::Resolved(decision, PermissionResolver::Phone))
    }

    /// 清扫过期未决项，返回需要落 `permission_resolved` 事件的 (id, 裁决) 列表。
    pub fn expire(&self, now_ms: u64) -> Vec<(String, PermissionDecision)> {
        let mut entries = self.entries.lock().expect("permission table poisoned");
        let mut expired = Vec::new();
        for (id, entry) in entries.iter_mut() {
            if entry.resolution.is_none() && now_ms > entry.expires_at_ms + SWEEP_GRACE_MS {
                entry.resolution =
                    Some((PermissionDecision::Deny, PermissionResolver::RelayExpired));
                expired.push((id.clone(), PermissionDecision::Deny));
            }
        }
        // 已裁决且早已过期的条目顺手回收，表不无限增长。
        entries.retain(|_, entry| {
            entry.resolution.is_none() || now_ms <= entry.expires_at_ms + RETAIN_MS
        });
        expired
    }
}

/// 清扫宽限：让 agent 侧 120s 本地超时通常先到，中继只兜底。
pub const SWEEP_GRACE_MS: u64 = 10_000;

/// 已裁决条目的保留时长（供迟到客户端读到 409 而不是 404）。
const RETAIN_MS: u64 = 300_000;

// ---------------------------------------------------------------------------
// 请求去重与会话索引。
// ---------------------------------------------------------------------------

/// 小容量 `client_request_id`/`client_msg_id` 去重：见过的直接回 accepted，不重复下发。
pub struct SeenRequests {
    seen: Mutex<(HashSet<String>, VecDeque<String>)>,
}

impl SeenRequests {
    /// 空表。
    pub fn empty() -> Self {
        Self {
            seen: Mutex::new((HashSet::new(), VecDeque::new())),
        }
    }

    /// 记录并返回是否重复。
    pub fn mark(&self, client_id: &str) -> bool {
        let mut seen = self.seen.lock().expect("seen requests poisoned");
        if seen.0.contains(client_id) {
            return true;
        }
        seen.0.insert(client_id.to_owned());
        seen.1.push_back(client_id.to_owned());
        if seen.1.len() > SEEN_CAP
            && let Some(evicted) = seen.1.pop_front()
        {
            seen.0.remove(&evicted);
        }
        false
    }
}

/// 去重容量：个人规模下远超一次通勤的重试数。
const SEEN_CAP: usize = 256;

/// 已知会话索引：手机发消息先验存在性（404 unknown-session）。
#[derive(Default)]
pub struct SessionIndex {
    ids: Mutex<HashSet<String>>,
}

impl SessionIndex {
    /// 空索引。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 登记会话 id。
    pub fn insert(&self, session_id: &str) {
        self.ids
            .lock()
            .expect("session index poisoned")
            .insert(session_id.to_owned());
    }

    /// 会话 id 是否已知。
    pub fn contains(&self, session_id: &str) -> bool {
        self.ids
            .lock()
            .expect("session index poisoned")
            .contains(session_id)
    }
}

// ---------------------------------------------------------------------------
// 聚合状态。
// ---------------------------------------------------------------------------

/// 中继全部共享状态。
pub struct RelayState {
    /// 事件时间线（手机 sync 的唯一事实源）。
    pub log: EventLog,
    /// agent 连接与命令通道。
    pub agents: AgentHub,
    /// 挂起权限表。
    pub permissions: PermissionTable,
    /// 请求幂等去重。
    pub seen_requests: SeenRequests,
    /// 已知会话。
    pub sessions: SessionIndex,
    persistence: Option<PersistHandle>,
}

impl RelayState {
    /// 无持久化的内存态。
    pub fn in_memory() -> Self {
        Self {
            log: EventLog::empty(),
            agents: AgentHub::empty(),
            permissions: PermissionTable::empty(),
            seen_requests: SeenRequests::empty(),
            sessions: SessionIndex::empty(),
            persistence: None,
        }
    }

    /// 带 JSONL 持久化构建（先重放再服务）。
    pub fn with_persistence(handle: PersistHandle, replayed: Vec<Event>) -> Self {
        let state = Self::in_memory();
        for kind in replayed.into_iter().map(|event| event.kind) {
            let _ = state.log.append(kind);
        }
        Self {
            persistence: Some(handle),
            ..state
        }
    }

    /// agent 上行事件的唯一入口：入日志、更新侧表、按需持久化。
    pub fn ingest_event(&self, kind: EventKind) -> Result<Event, RelayError> {
        if let EventKind::PermissionRequested {
            permission_id,
            expires_at_ms,
            ..
        } = &kind
        {
            self.permissions
                .register(permission_id.clone(), *expires_at_ms);
        }
        if let EventKind::SessionCreated { session_id, .. } = &kind {
            self.sessions.insert(session_id);
        }
        self.append_event(kind)
    }

    /// 中继自身产生的事件（agent_status、turn_started、permission_resolved 等）。
    pub fn append_event(&self, kind: EventKind) -> Result<Event, RelayError> {
        let event = self.log.append(kind)?;
        if let Some(persistence) = &self.persistence {
            persistence.append(&event);
        }
        Ok(event)
    }

    /// agent 上线：登记连接并广播 `agent_status`；返回连接 id 供摘除时核对。
    pub fn mark_agent_online(&self, agent_id: &str, sender: Sender<DownlinkMessage>) -> u64 {
        let connection_id = self.agents.attach(agent_id.to_owned(), sender);
        let _ = self.append_event(EventKind::AgentStatus {
            online: true,
            agent_id: agent_id.to_owned(),
        });
        connection_id
    }

    /// agent 掉线：仅当仍是当前连接时摘除并广播。
    pub fn mark_agent_offline(&self, connection_id: u64) {
        self.agents.detach(connection_id);
        if !self.agents.online() {
            let _ = self.append_event(EventKind::AgentStatus {
                online: false,
                agent_id: String::new(),
            });
        }
    }

    /// 清扫过期权限并落 `permission_resolved`（relay-expired）事件。
    pub fn expire_permissions(&self) {
        for (permission_id, decision) in self.permissions.expire(now_ms()) {
            let _ = self.append_event(EventKind::PermissionResolved {
                permission_id,
                decision,
                resolved_by: PermissionResolver::RelayExpired,
            });
        }
    }
}

/// 便捷别名：各线程共享的状态句柄。
pub type SharedState = Arc<RelayState>;

#[cfg(test)]
mod tests {
    use super::*;
    use tela_cc_protocol::NoticeLevel;

    fn notice(text: &str) -> EventKind {
        EventKind::Notice {
            level: NoticeLevel::Info,
            text: text.to_owned(),
        }
    }

    #[test]
    fn event_log_seq_is_monotonic_without_gaps() {
        let log = EventLog::empty();
        let first = log.append(notice("a")).expect("append");
        let second = log.append(notice("b")).expect("append");
        assert_eq!(first.seq, 1);
        assert_eq!(second.seq, 2);
        assert_eq!(log.latest_seq(), 2);
        assert_eq!(log.peek_next_seq(), 3);
    }

    #[test]
    fn event_log_replay_continues_seq() {
        let replayed = vec![Event {
            seq: 41,
            ts_ms: 1,
            kind: notice("old"),
        }];
        let log = EventLog::from_replay(replayed);
        assert_eq!(log.latest_seq(), 41);
        let next = log.append(notice("new")).expect("append");
        assert_eq!(next.seq, 42);
    }

    #[test]
    fn event_log_trims_to_cap_and_since_clamps_to_floor() {
        let log = EventLog::empty();
        for index in 0..(EVENT_LOG_CAP as u64 + 50) {
            log.append(notice(&index.to_string())).expect("append");
        }
        assert_eq!(log.latest_seq(), EVENT_LOG_CAP as u64 + 50);
        let (page, _) = log.since(0, SYNC_LIMIT_FOR_TEST);
        let floor = page.first().expect("page").seq;
        assert_eq!(floor, 51, "oldest retained event is trimmed to the cap");

        // since 落到 floor 之前：静默钳到 floor，返回现存最早事件。
        let (clamped, _) = log.since(0, 10);
        assert_eq!(clamped.first().expect("clamped").seq, floor);

        let (tail, more) = log.since(EVENT_LOG_CAP as u64 + 40, 5);
        assert_eq!(tail.len(), 5);
        assert!(more, "remaining events beyond the page limit");
    }

    const SYNC_LIMIT_FOR_TEST: usize = 500;

    #[test]
    fn oversize_events_are_rejected() {
        let log = EventLog::empty();
        let huge = EventKind::Notice {
            level: NoticeLevel::Info,
            text: "x".repeat(MAX_EVENT_BYTES),
        };
        assert!(log.append(huge).is_err());
    }

    #[test]
    fn permission_table_decide_is_single_direction() {
        let table = PermissionTable::empty();
        table.register("p1".to_owned(), 1_000);
        let outcome = table
            .decide("p1", PermissionDecision::Allow, 900)
            .expect("decide");
        assert_eq!(
            outcome,
            DecideOutcome::Resolved(PermissionDecision::Allow, PermissionResolver::Phone)
        );
        let again = table
            .decide("p1", PermissionDecision::Deny, 950)
            .expect("decide again");
        assert_eq!(
            again,
            DecideOutcome::AlreadyResolved(PermissionDecision::Allow, PermissionResolver::Phone)
        );
        assert!(
            table
                .decide("missing", PermissionDecision::Allow, 0)
                .is_err()
        );
    }

    #[test]
    fn permission_table_expires_only_past_grace() {
        let table = PermissionTable::empty();
        table.register("p1".to_owned(), 1_000);
        assert!(table.expire(1_000 + SWEEP_GRACE_MS).is_empty());
        let expired = table.expire(1_000 + SWEEP_GRACE_MS + 1);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, "p1");
        // 过期后手机再裁决：读到的是已裁决状态（409 而非二次裁决）。
        let late = table
            .decide("p1", PermissionDecision::Allow, 1_000 + SWEEP_GRACE_MS + 2)
            .expect("decide after sweep");
        assert!(matches!(late, DecideOutcome::AlreadyResolved(_, _)));
    }

    #[test]
    fn seen_requests_marks_duplicates_within_capacity() {
        let seen = SeenRequests::empty();
        assert!(!seen.mark("a"));
        assert!(seen.mark("a"));
        for index in 0..(SEEN_CAP as u64 + 10) {
            seen.mark(&format!("id-{index}"));
        }
        // 最早的 "a" 已被挤出容量，重新视为新请求。
        assert!(!seen.mark("a"));
    }

    #[test]
    fn ingest_event_registers_permission_and_session_side_tables() {
        let state = RelayState::in_memory();
        state
            .ingest_event(EventKind::SessionCreated {
                session_id: "s1".to_owned(),
                title: None,
            })
            .expect("ingest");
        assert!(state.sessions.contains("s1"));
        state
            .ingest_event(EventKind::PermissionRequested {
                permission_id: "perm-1".to_owned(),
                session_id: "s1".to_owned(),
                turn_id: "t1".to_owned(),
                tool_name: "Bash".to_owned(),
                input_summary: "echo hi".to_owned(),
                expires_at_ms: 9_999,
            })
            .expect("ingest");
        assert!(
            state
                .permissions
                .decide("perm-1", PermissionDecision::Deny, 0)
                .is_ok()
        );
    }
}
