//! fake 模式：不起 claude 子进程，用内置剧本驱动全链路联调。
//!
//! 剧本与真实 manager 的事件形状完全一致（TurnStarted → AssistantText → ToolUse →
//! PermissionRequested → …→ PermissionResolved → ToolResult → TurnResult），手机端与
//! 中继无法区分 fake 与真实来源。

use std::collections::HashMap;

use tela_cc_protocol::{
    DownlinkMessage, EventKind, PERMISSION_TIMEOUT_MS, PermissionDecision, PermissionResolver,
};

/// fake 后端状态机。
pub struct FakeAgent {
    sessions: HashMap<String, FakeSession>,
    counter: u64,
}

struct FakeSession {
    turns: HashMap<String, FakeTurn>,
}

struct FakeTurn {
    _prompt: String,
    permission_id: Option<String>,
    expires_at_ms: u64,
}

impl FakeAgent {
    /// 创建空剧本状态。
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            counter: 0,
        }
    }

    /// 处理一条下行命令，产出即时事件。
    pub fn handle_command(&mut self, command: &DownlinkMessage, now_ms: u64) -> Vec<EventKind> {
        match command {
            DownlinkMessage::CreateSession { .. } => {
                self.counter += 1;
                let session_id = format!("fake-{}", self.counter);
                self.sessions.insert(
                    session_id.clone(),
                    FakeSession {
                        turns: HashMap::new(),
                    },
                );
                vec![EventKind::SessionCreated {
                    session_id,
                    title: Some("fake 会话".to_owned()),
                }]
            }
            DownlinkMessage::RunTurn {
                session_id,
                turn_id,
                prompt,
                ..
            } => {
                let Some(session) = self.sessions.get_mut(session_id) else {
                    return vec![EventKind::Notice {
                        level: tela_cc_protocol::NoticeLevel::Error,
                        text: format!("未知会话 {session_id}"),
                    }];
                };
                let preview = truncate(prompt, 40);
                let permission_id = format!("fake-perm-{turn_id}");
                session.turns.insert(
                    turn_id.clone(),
                    FakeTurn {
                        _prompt: prompt.clone(),
                        permission_id: Some(permission_id.clone()),
                        expires_at_ms: now_ms + PERMISSION_TIMEOUT_MS,
                    },
                );
                // turn_started 由中继在受理消息时落；agent 不重复。
                vec![
                    EventKind::AssistantText {
                        session_id: session_id.clone(),
                        turn_id: turn_id.clone(),
                        message_id: format!("{turn_id}-m1"),
                        text: format!("已收到：{preview}"),
                    },
                    EventKind::ToolUse {
                        session_id: session_id.clone(),
                        turn_id: turn_id.clone(),
                        tool_use_id: format!("{turn_id}-call"),
                        tool_name: "Bash".to_owned(),
                        input_json: r#"{"command":"echo cc-remote"}"#.to_owned(),
                    },
                    EventKind::PermissionRequested {
                        permission_id,
                        session_id: session_id.clone(),
                        turn_id: turn_id.clone(),
                        tool_name: "Bash".to_owned(),
                        input_summary: r#"{"command":"echo cc-remote"}"#.to_owned(),
                        expires_at_ms: now_ms + PERMISSION_TIMEOUT_MS,
                    },
                ]
            }
            DownlinkMessage::PermissionDecision {
                permission_id,
                decision,
            } => {
                let mut hit: Option<(String, String)> = None;
                for (session_id, session) in self.sessions.iter_mut() {
                    for (turn_id, turn) in session.turns.iter_mut() {
                        if turn.permission_id.as_deref() == Some(permission_id.as_str()) {
                            turn.permission_id = None;
                            hit = Some((session_id.clone(), turn_id.clone()));
                            break;
                        }
                    }
                    if hit.is_some() {
                        break;
                    }
                }
                match hit {
                    Some((session_id, turn_id)) => self.finish_turn(
                        &session_id,
                        &turn_id,
                        *decision,
                        PermissionResolver::Phone,
                    ),
                    None => Vec::new(),
                }
            }
            DownlinkMessage::CancelTurn { .. } => Vec::new(),
            DownlinkMessage::HelloOk { .. }
            | DownlinkMessage::Pong
            | DownlinkMessage::Error { .. } => Vec::new(),
        }
    }

    /// 周期驱动：权限超时按 agent-timeout 收尾。
    pub fn tick(&mut self, now_ms: u64) -> Vec<EventKind> {
        let mut expired = Vec::new();
        for (session_id, session) in self.sessions.iter_mut() {
            for (turn_id, turn) in session.turns.iter_mut() {
                if turn.permission_id.is_some() && turn.expires_at_ms <= now_ms {
                    turn.permission_id = None;
                    expired.push((session_id.clone(), turn_id.clone()));
                }
            }
        }
        let mut events = Vec::new();
        for (session_id, turn_id) in expired {
            events.extend(self.finish_turn(
                &session_id,
                &turn_id,
                PermissionDecision::Deny,
                PermissionResolver::AgentTimeout,
            ));
        }
        events
    }

    fn finish_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        decision: PermissionDecision,
        resolved_by: PermissionResolver,
    ) -> Vec<EventKind> {
        let denied = decision == PermissionDecision::Deny;
        let mut events = Vec::new();
        // 手机裁决的 permission_resolved 由中继落事件；agent 只在本地超时时补
        // agent-timeout 裁决（中继不知道 agent 已经放弃等待）。
        if resolved_by == PermissionResolver::AgentTimeout {
            events.push(EventKind::PermissionResolved {
                permission_id: format!("fake-perm-{turn_id}"),
                decision,
                resolved_by,
            });
        }
        events.push(EventKind::ToolResult {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            tool_use_id: format!("{turn_id}-call"),
            content: if denied {
                "等待手机批准超时/被拒绝，未执行".to_owned()
            } else {
                "cc-remote".to_owned()
            },
            is_error: denied,
        });
        events.push(EventKind::TurnResult {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            subtype: "success".to_owned(),
            cost_usd: Some(0.01),
            duration_ms: Some(64),
            session_id_confirmed: None,
        });
        events
    }
}

impl Default for FakeAgent {
    fn default() -> Self {
        Self::new()
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let truncated: String = text.chars().take(limit).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_turn_command(session_id: &str, turn_id: &str, prompt: &str) -> DownlinkMessage {
        DownlinkMessage::RunTurn {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            prompt: prompt.to_owned(),
            resume: false,
        }
    }

    #[test]
    fn create_and_run_turn_emit_the_scripted_sequence() {
        let mut agent = FakeAgent::new();
        let created = agent.handle_command(
            &DownlinkMessage::CreateSession {
                client_request_id: "c1".to_owned(),
            },
            1_000,
        );
        assert!(matches!(
            created.as_slice(),
            [EventKind::SessionCreated { session_id, .. }] if session_id == "fake-1"
        ));

        let events = agent.handle_command(&run_turn_command("fake-1", "t1", "跑一下测试"), 2_000);
        let kinds: Vec<&str> = events
            .iter()
            .map(|event| match event {
                EventKind::TurnStarted { .. } => "turn_started",
                EventKind::AssistantText { .. } => "assistant_text",
                EventKind::ToolUse { .. } => "tool_use",
                EventKind::PermissionRequested { .. } => "permission_requested",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["assistant_text", "tool_use", "permission_requested"]
        );
    }

    #[test]
    fn phone_decision_finishes_the_turn() {
        let mut agent = FakeAgent::new();
        agent.handle_command(
            &DownlinkMessage::CreateSession {
                client_request_id: "c1".to_owned(),
            },
            1_000,
        );
        agent.handle_command(&run_turn_command("fake-1", "t1", "hi"), 2_000);
        let events = agent.handle_command(
            &DownlinkMessage::PermissionDecision {
                permission_id: "fake-perm-t1".to_owned(),
                decision: PermissionDecision::Allow,
            },
            3_000,
        );
        // 手机裁决的 permission_resolved 由中继落事件；agent 不重复上行。
        assert!(
            !events
                .iter()
                .any(|event| { matches!(event, EventKind::PermissionResolved { .. }) })
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, EventKind::TurnResult { .. }))
        );
    }

    #[test]
    fn timeout_decision_finishes_the_turn_on_tick() {
        let mut agent = FakeAgent::new();
        agent.handle_command(
            &DownlinkMessage::CreateSession {
                client_request_id: "c1".to_owned(),
            },
            1_000,
        );
        agent.handle_command(&run_turn_command("fake-1", "t1", "hi"), 2_000);
        assert!(agent.tick(3_000).is_empty(), "未超时");
        let events = agent.tick(2_000 + PERMISSION_TIMEOUT_MS + 1);
        assert!(events.iter().any(|event| matches!(
            event,
            EventKind::PermissionResolved {
                resolved_by: PermissionResolver::AgentTimeout,
                ..
            }
        )));
    }
}
