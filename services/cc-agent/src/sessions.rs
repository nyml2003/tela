//! 会话管理：每会话一个 claude 子进程，stdout 行到事件流的映射，以及权限挂起仲裁。
//!
//! 手机侧的 `session_id` 是 agent 生成的稳定 id，与 CLI 的真实 session id 解耦；真实 id
//! 只存在于 `real_session_id` 映射里，供进程死亡后 `--resume` 恢复，并经
//! `session_id_confirmed` 字段回填到事件流供调试。权限三方竞态（手机 / agent 超时 /
//! 中继清扫）在 agent 侧收敛为"pending 条目存在与否"：先移除者裁决，后到者自然忽略。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tela_cc_protocol::{
    EventKind, NoticeLevel, PERMISSION_TIMEOUT_MS, PermissionDecision, PermissionResolver,
    UplinkMessage,
};

use crate::claude::{self, ClaudeLine};
use crate::config::AgentConfig;

/// 读线程单行缓冲上限；超过的行按噪音丢弃，防大输出撑爆内存。
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// 会话内共享状态；被主线程（命令分发）与各会话读线程并发访问。
#[derive(Default)]
struct AgentShared {
    sessions: HashMap<String, Session>,
    /// permission_id → 所属会话；手机决策的快速路由。
    permission_index: HashMap<String, String>,
}

struct Session {
    /// 手机侧稳定 id（HashMap 键即 id；字段仅作诊断存档）。
    _id: String,
    /// CLI 的真实 session id（首个 init 回填）。
    real_session_id: Option<String>,
    stdin: Option<ChildStdin>,
    child: Option<Child>,
    mapper: TurnMapper,
    active_turn: Option<String>,
    pending: HashMap<String, PendingPermission>,
    /// 子进程已退出（EOF）；下一回合将以 `--resume` 重建。
    dead: bool,
}

struct PendingPermission {
    request_id: String,
    tool_use_id: Option<String>,
    expires_at_ms: u64,
}

/// 会话与权限的管理器。
pub struct SessionManager {
    config: AgentConfig,
    shared: Arc<Mutex<AgentShared>>,
    uplink: SyncSender<UplinkMessage>,
}

impl SessionManager {
    /// 创建管理器；`uplink` 用于事件上行。
    pub fn new(config: AgentConfig, uplink: SyncSender<UplinkMessage>) -> Self {
        Self {
            config,
            shared: Arc::new(Mutex::new(AgentShared::default())),
            uplink,
        }
    }

    /// 处理一条中继下行命令。
    pub fn handle_command(&self, command: tela_cc_protocol::DownlinkMessage) {
        use tela_cc_protocol::DownlinkMessage;
        match command {
            DownlinkMessage::CreateSession { client_request_id } => {
                let session_id = fresh_id("sess");
                self.emit(EventKind::SessionCreated {
                    session_id: session_id.clone(),
                    title: None,
                });
                self.shared
                    .lock()
                    .expect("agent state lock")
                    .sessions
                    .insert(
                        session_id.clone(),
                        Session {
                            _id: session_id.clone(),
                            real_session_id: None,
                            stdin: None,
                            child: None,
                            mapper: TurnMapper::new(session_id),
                            active_turn: None,
                            pending: HashMap::new(),
                            dead: false,
                        },
                    );
                let _ = client_request_id;
            }
            DownlinkMessage::RunTurn {
                session_id,
                turn_id,
                prompt,
                resume,
            } => self.run_turn(session_id, turn_id, prompt, resume),
            DownlinkMessage::PermissionDecision {
                permission_id,
                decision,
            } => self.permission_decision(permission_id, decision),
            // 取消回合是 M2 的尽力语义；v1 忽略。
            DownlinkMessage::CancelTurn { .. } => {}
            DownlinkMessage::HelloOk { .. }
            | DownlinkMessage::Pong
            | DownlinkMessage::Error { .. } => {}
        }
    }

    /// 权限超时清扫；由主循环周期驱动。
    pub fn sweep_expired(&self, now_ms: u64) {
        let expired = {
            let mut shared = self.shared.lock().expect("agent state lock");
            let mut expired = Vec::new();
            for session in shared.sessions.values_mut() {
                let ids: Vec<String> = session
                    .pending
                    .iter()
                    .filter(|(_, pending)| pending.expires_at_ms <= now_ms)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in ids {
                    if let Some(pending) = session.pending.remove(&id) {
                        if write_stdin_line(
                            session,
                            &claude::permission_response_line(
                                &pending.request_id,
                                pending.tool_use_id.as_deref(),
                                false,
                                "等待手机批准超时，默认拒绝",
                            ),
                        )
                        .is_err()
                        {
                            eprintln!("[cc-agent] 写权限超时应答失败");
                        }
                        expired.push(id);
                    }
                }
            }
            for id in &expired {
                shared.permission_index.remove(id);
            }
            expired
        };
        for id in expired {
            self.emit(EventKind::PermissionResolved {
                permission_id: id,
                decision: PermissionDecision::Deny,
                resolved_by: PermissionResolver::AgentTimeout,
            });
        }
    }

    fn run_turn(&self, session_id: String, turn_id: String, prompt: String, resume: bool) {
        let Some(cwd) = self.config.default_cwd() else {
            self.emit(EventKind::Notice {
                level: NoticeLevel::Error,
                text: "CC_AGENT_CWDS 未配置，拒绝执行回合".to_owned(),
            });
            return;
        };
        let mut failure: Option<EventKind> = None;
        {
            let mut shared = self.shared.lock().expect("agent state lock");
            let Some(session) = shared.sessions.get_mut(&session_id) else {
                return;
            };
            if session.stdin.is_none() || session.dead {
                match spawn_claude(&self.config.claude, cwd, session.real_session_id.as_deref()) {
                    Ok((child, stdin, stdout)) => {
                        session.stdin = Some(stdin);
                        session.child = Some(child);
                        session.dead = false;
                        spawn_reader_thread(
                            Arc::clone(&self.shared),
                            session_id.clone(),
                            stdout,
                            self.uplink.clone(),
                        );
                    }
                    Err(error) => {
                        session.dead = true;
                        failure = Some(EventKind::Notice {
                            level: NoticeLevel::Error,
                            text: format!("启动 claude 子进程失败: {error}"),
                        });
                    }
                }
            }
            if failure.is_none() {
                let _ = resume;
                session.mapper.begin_turn(&turn_id);
                session.active_turn = Some(turn_id.clone());
                let line = claude::user_turn_line(&prompt);
                if write_stdin_line(session, &line).is_err() {
                    failure = Some(EventKind::Notice {
                        level: NoticeLevel::Error,
                        text: "向 claude stdin 写入回合失败".to_owned(),
                    });
                }
            }
        }
        match failure {
            Some(notice) => self.emit(notice),
            None => self.emit(EventKind::TurnStarted {
                session_id,
                turn_id,
                user_text: prompt,
            }),
        }
    }

    fn permission_decision(&self, permission_id: String, decision: PermissionDecision) {
        {
            let mut shared = self.shared.lock().expect("agent state lock");
            let Some(session_id) = shared.permission_index.remove(&permission_id) else {
                return; // 已被超时或中继清扫裁决；幂等忽略。
            };
            let Some(session) = shared.sessions.get_mut(&session_id) else {
                return;
            };
            let Some(pending) = session.pending.remove(&permission_id) else {
                return;
            };
            let message = match decision {
                PermissionDecision::Allow => "",
                PermissionDecision::Deny => "用户在手机上拒绝了该操作",
            };
            if write_stdin_line(
                session,
                &claude::permission_response_line(
                    &pending.request_id,
                    pending.tool_use_id.as_deref(),
                    decision == PermissionDecision::Allow,
                    message,
                ),
            )
            .is_err()
            {
                eprintln!("[cc-agent] 写权限应答失败");
            }
        }
        // 手机裁决的 permission_resolved 由中继落事件；agent 只负责把决策写给 CLI 子进程。
    }

    fn emit(&self, kind: EventKind) {
        let _ = self.uplink.try_send(UplinkMessage::Event { event: kind });
    }
}

/// 向会话 stdin 写一行；失败标记会话死亡（进程已退出，stdin 管道断裂）。
fn write_stdin_line(session: &mut Session, line: &str) -> Result<(), ()> {
    let Some(stdin) = session.stdin.as_mut() else {
        return Err(());
    };
    match stdin
        .write_all(line.as_bytes())
        .and_then(|()| stdin.write_all(b"\n"))
    {
        Ok(()) => Ok(()),
        Err(_) => {
            session.dead = true;
            Err(())
        }
    }
}

/// 启动 claude 子进程；`resume_session` 存在时附加 `--resume`。
fn spawn_claude(
    claude: &str,
    cwd: &std::path::Path,
    resume_session: Option<&str>,
) -> std::io::Result<(Child, ChildStdin, std::process::ChildStdout)> {
    let mut command = Command::new(claude);
    command
        .arg("-p")
        .arg("--input-format")
        .arg("stream-json")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--permission-prompt-tool")
        .arg("stdio")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(session_id) = resume_session {
        command.arg("--resume").arg(session_id);
    }
    let mut child = command.spawn()?;
    let stdin = child.stdin.take().expect("claude stdin piped");
    let stdout = child.stdout.take().expect("claude stdout piped");
    Ok((child, stdin, stdout))
}

/// 会话读线程：逐行解析 stdout，映射为事件上行；ControlRequest 登记权限挂起。
fn spawn_reader_thread(
    shared: Arc<Mutex<AgentShared>>,
    session_id: String,
    stdout: std::process::ChildStdout,
    uplink: SyncSender<UplinkMessage>,
) {
    std::thread::Builder::new()
        .name(format!("cc-agent-session-{session_id}"))
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                // read_line 不受 MAX_LINE_BYTES 硬限（lines() 同理）；大行先按上限截断。
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
                if line.len() > MAX_LINE_BYTES {
                    continue;
                }
                for parsed in claude::parse_line(&line) {
                    if let ClaudeLine::ControlRequest {
                        request_id,
                        tool_use_id,
                        tool_name,
                        input_json,
                    } = parsed
                    {
                        let permission_id = fresh_id("perm");
                        let now = now_ms();
                        let registered = {
                            let mut shared = shared.lock().expect("agent state lock");
                            let turn_id = shared
                                .sessions
                                .get(&session_id)
                                .and_then(|session| session.active_turn.clone())
                                .unwrap_or_else(|| "unknown".to_owned());
                            shared
                                .permission_index
                                .insert(permission_id.clone(), session_id.clone());
                            match shared.sessions.get_mut(&session_id) {
                                Some(session) => {
                                    session.pending.insert(
                                        permission_id.clone(),
                                        PendingPermission {
                                            request_id,
                                            tool_use_id: tool_use_id.clone(),
                                            expires_at_ms: now + PERMISSION_TIMEOUT_MS,
                                        },
                                    );
                                    (turn_id, now)
                                }
                                None => break,
                            }
                        };
                        let _ = uplink.try_send(UplinkMessage::Event {
                            event: EventKind::PermissionRequested {
                                permission_id,
                                session_id: session_id.clone(),
                                turn_id: registered.0,
                                tool_name,
                                input_summary: truncate_chars(&input_json, 200),
                                expires_at_ms: registered.1 + PERMISSION_TIMEOUT_MS,
                            },
                        });
                        continue;
                    }
                    let events = {
                        let mut shared = shared.lock().expect("agent state lock");
                        let Some(session) = shared.sessions.get_mut(&session_id) else {
                            break;
                        };
                        let events = session.mapper.apply(&parsed, now_ms());
                        if let ClaudeLine::Init { session_id: real } = &parsed {
                            session.real_session_id = Some(real.clone());
                        }
                        events
                    };
                    for event in events {
                        let _ = uplink.try_send(UplinkMessage::Event { event });
                    }
                }
            }
            // stdout EOF：进程已退出。回收子进程并标记死亡，供下一回合 --resume 重建。
            let mut shared = shared.lock().expect("agent state lock");
            let notice = if let Some(session) = shared.sessions.get_mut(&session_id) {
                session.dead = true;
                session.stdin = None;
                if let Some(mut child) = session.child.take() {
                    let _ = child.wait();
                }
                Some(EventKind::Notice {
                    level: NoticeLevel::Info,
                    text: format!("会话 {session_id} 的 claude 进程已退出"),
                })
            } else {
                None
            };
            drop(shared);
            if let Some(event) = notice {
                let _ = uplink.try_send(UplinkMessage::Event { event });
            }
        })
        .expect("spawn session reader thread");
}

/// 把 CLI 消息行映射为事件；同会话跨回合复用（`real_session_id` 与确认位保留）。
pub struct TurnMapper {
    session_id: String,
    turn_id: String,
    real_session_id: Option<String>,
    confirmed_real: bool,
    current_message_id: Option<String>,
    accumulated_text: String,
}

impl TurnMapper {
    /// 以手机侧稳定会话 id 构造。
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            turn_id: String::new(),
            real_session_id: None,
            confirmed_real: false,
            current_message_id: None,
            accumulated_text: String::new(),
        }
    }

    /// 开始新回合：重置文本累积，保留会话级状态。
    pub fn begin_turn(&mut self, turn_id: &str) {
        self.turn_id = turn_id.to_owned();
        self.current_message_id = None;
        self.accumulated_text.clear();
    }

    /// CLI 真实 session id（首个 init 后可用；测试使用）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn real_session_id(&self) -> Option<&str> {
        self.real_session_id.as_deref()
    }

    /// 应用一行解析结果，产出待上行的事件。
    pub fn apply(&mut self, line: &ClaudeLine, _now_ms: u64) -> Vec<EventKind> {
        match line {
            ClaudeLine::Init { session_id } => {
                self.real_session_id = Some(session_id.clone());
                Vec::new()
            }
            ClaudeLine::AssistantBlock {
                message_id,
                block: claude::ClaudeBlock::Text(text),
            } => {
                if self.current_message_id.as_deref() != Some(message_id) {
                    self.current_message_id = Some(message_id.clone());
                    self.accumulated_text.clear();
                }
                self.accumulated_text.push_str(text);
                vec![EventKind::AssistantText {
                    session_id: self.session_id.clone(),
                    turn_id: self.turn_id.clone(),
                    message_id: message_id.clone(),
                    text: self.accumulated_text.clone(),
                }]
            }
            ClaudeLine::AssistantBlock {
                block:
                    claude::ClaudeBlock::ToolUse {
                        tool_use_id,
                        tool_name,
                        input_json,
                    },
                ..
            } => vec![EventKind::ToolUse {
                session_id: self.session_id.clone(),
                turn_id: self.turn_id.clone(),
                tool_use_id: tool_use_id.clone(),
                tool_name: tool_name.clone(),
                input_json: input_json.clone(),
            }],
            ClaudeLine::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => vec![EventKind::ToolResult {
                session_id: self.session_id.clone(),
                turn_id: self.turn_id.clone(),
                tool_use_id: tool_use_id.clone(),
                content: truncate_chars(content, 4000),
                is_error: *is_error,
            }],
            ClaudeLine::TurnFinished {
                subtype,
                session_id,
                cost_usd,
                duration_ms,
            } => {
                let confirmed = match (session_id.as_deref(), self.confirmed_real) {
                    (Some(real), false) if Some(real) == self.real_session_id.as_deref() => {
                        self.confirmed_real = true;
                        Some(real.to_owned())
                    }
                    (Some(real), false) => {
                        // result 先于 init 携带 id 的兜底路径。
                        self.real_session_id = Some(real.to_owned());
                        self.confirmed_real = true;
                        Some(real.to_owned())
                    }
                    _ => None,
                };
                self.current_message_id = None;
                self.accumulated_text.clear();
                vec![EventKind::TurnResult {
                    session_id: self.session_id.clone(),
                    turn_id: self.turn_id.clone(),
                    subtype: subtype.clone(),
                    cost_usd: *cost_usd,
                    duration_ms: *duration_ms,
                    session_id_confirmed: confirmed,
                }]
            }
            ClaudeLine::ControlRequest { .. } | ClaudeLine::Ignore => Vec::new(),
        }
    }
}

/// 全局 id 计数器（进程内唯一；配合 pid 与毫秒时间戳跨进程去重）。
static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn fresh_id(prefix: &str) -> String {
    let sequence = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{sequence}", now_ms())
}

/// 当前 UTC 毫秒。
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 按字符数截断（中文安全），附加省略号。
fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let truncated: String = text.chars().take(limit).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapper_accumulates_text_blocks_per_message() {
        let mut mapper = TurnMapper::new("sess-1".to_owned());
        mapper.begin_turn("turn-1");
        let first = mapper.apply(
            &ClaudeLine::AssistantBlock {
                message_id: "msg_1".to_owned(),
                block: claude::ClaudeBlock::Text("第一段".to_owned()),
            },
            0,
        );
        assert_eq!(first.len(), 1);
        let EventKind::AssistantText { text, .. } = &first[0] else {
            panic!("expected assistant text");
        };
        assert_eq!(text, "第一段");

        let second = mapper.apply(
            &ClaudeLine::AssistantBlock {
                message_id: "msg_1".to_owned(),
                block: claude::ClaudeBlock::Text("第二段".to_owned()),
            },
            0,
        );
        let EventKind::AssistantText { text, .. } = &second[0] else {
            panic!("expected assistant text");
        };
        assert_eq!(text, "第一段第二段");

        // 新消息重新累积。
        let third = mapper.apply(
            &ClaudeLine::AssistantBlock {
                message_id: "msg_2".to_owned(),
                block: claude::ClaudeBlock::Text("新消息".to_owned()),
            },
            0,
        );
        let EventKind::AssistantText {
            text, message_id, ..
        } = &third[0]
        else {
            panic!("expected assistant text");
        };
        assert_eq!(text, "新消息");
        assert_eq!(message_id, "msg_2");
    }

    #[test]
    fn mapper_confirms_real_session_id_once() {
        let mut mapper = TurnMapper::new("sess-1".to_owned());
        mapper.begin_turn("turn-1");
        assert!(
            mapper
                .apply(
                    &ClaudeLine::Init {
                        session_id: "real-9".to_owned(),
                    },
                    0
                )
                .is_empty()
        );
        assert_eq!(mapper.real_session_id(), Some("real-9"));

        let first = mapper.apply(
            &ClaudeLine::TurnFinished {
                subtype: "success".to_owned(),
                session_id: Some("real-9".to_owned()),
                cost_usd: Some(0.02),
                duration_ms: Some(1200),
            },
            0,
        );
        let EventKind::TurnResult {
            session_id_confirmed,
            ..
        } = &first[0]
        else {
            panic!("expected turn result");
        };
        assert_eq!(session_id_confirmed.as_deref(), Some("real-9"));

        mapper.begin_turn("turn-2");
        let second = mapper.apply(
            &ClaudeLine::TurnFinished {
                subtype: "success".to_owned(),
                session_id: Some("real-9".to_owned()),
                cost_usd: None,
                duration_ms: None,
            },
            0,
        );
        let EventKind::TurnResult {
            session_id_confirmed,
            ..
        } = &second[0]
        else {
            panic!("expected turn result");
        };
        assert!(session_id_confirmed.is_none(), "每会话只确认一次");
    }

    #[test]
    fn mapper_uses_phone_side_session_id_in_events() {
        let mut mapper = TurnMapper::new("sess-phone".to_owned());
        mapper.begin_turn("turn-1");
        let events = mapper.apply(
            &ClaudeLine::ToolResult {
                tool_use_id: "call-1".to_owned(),
                content: "hello".to_owned(),
                is_error: false,
            },
            0,
        );
        let EventKind::ToolResult { session_id, .. } = &events[0] else {
            panic!("expected tool result");
        };
        assert_eq!(session_id, "sess-phone");
    }
}
