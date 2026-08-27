//! 领域层：会话与聊天状态的纯 reducer。
//!
//! 唯一入口是 [`apply_event`]：按 `seq` 升序喂入中继事件，返回状态是否变化。重复投递
//! （分页重叠、重连重放）由各 item 的稳定 id 幂等吸收；乐观消息（本地先显示的用户
//! 输入）在 `turn_started` 到达时由应用层对账，本层只认事件。

use std::collections::HashMap;

use tela_cc_protocol::{Event, EventKind, PermissionDecision, PermissionResolver};

/// 一个会话的列表摘要。
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub id: String,
    pub title: String,
    /// 列表预览：最近一条用户输入。
    pub preview: String,
    /// 最近事件的 `seq`，列表按它倒序。
    pub last_seq: u64,
    /// 是否有未完成的回合（发送中标记）。
    pub turn_active: bool,
}

/// 聊天流中的一个展示项。
#[derive(Clone, Debug, PartialEq)]
pub enum ChatItem {
    /// 用户输入（`pending` 由应用层的乐观消息维护；事件侧恒为已确认）。
    UserText { turn_id: String, text: String },
    /// 助手文本；同 `message_id` 全量替换（流式增量是 M2 的兼容扩展）。
    AssistantText {
        turn_id: String,
        message_id: String,
        text: String,
    },
    /// 工具调用卡。
    ToolUse {
        turn_id: String,
        tool_use_id: String,
        tool_name: String,
        input_json: String,
    },
    /// 工具结果块。
    ToolResult {
        turn_id: String,
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    /// 回合收尾行（时长/费用/子类型）。
    TurnResult {
        turn_id: String,
        subtype: String,
        cost_usd: Option<f64>,
        duration_ms: Option<u64>,
    },
    /// 全局通知（落在当前打开的会话里或列表页脚）。
    Notice { text: String },
}

/// 一张权限卡：待决或已决。
#[derive(Clone, Debug, PartialEq)]
pub struct PermissionCard {
    pub permission_id: String,
    pub tool_name: String,
    pub input_summary: String,
    pub expires_at_ms: u64,
    /// `None` 表示仍在等待手机决策。
    pub resolution: Option<(PermissionDecision, PermissionResolver)>,
}

/// 一个会话的聊天状态。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatState {
    pub items: Vec<ChatItem>,
    /// 当前（或最近一张）权限卡。
    pub permission: Option<PermissionCard>,
}

/// 世界状态：事件流的本地投影。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct World {
    /// 已确认的最大事件 `seq`。
    pub cursor: u64,
    pub sessions: Vec<Session>,
    pub chats: HashMap<String, ChatState>,
    pub agent_online: bool,
    /// 最近的全局通知（列表页脚展示，容量 8）。
    pub notices: Vec<String>,
}

/// 事件是否改变了世界状态。
pub fn apply_event(world: &mut World, event: &Event) -> bool {
    if event.seq <= world.cursor {
        // 重复或乱序投递：游标只认单调前进。
        return false;
    }
    world.cursor = event.seq;
    let changed = apply_kind(world, &event.kind);
    if let Some(session_id) = event.kind.session_id() {
        if let Some(session) = world
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.last_seq = event.seq;
        } else {
            world.sessions.push(Session {
                id: session_id.to_owned(),
                title: session_id.to_owned(),
                preview: String::new(),
                last_seq: event.seq,
                turn_active: false,
            });
        }
    }
    changed
}

fn apply_kind(world: &mut World, kind: &EventKind) -> bool {
    match kind {
        EventKind::SessionCreated { session_id, title } => {
            let title = title
                .as_deref()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or("新会话");
            if let Some(session) = session_mut(world, session_id) {
                session.title = title.to_owned();
                return true;
            }
            world.sessions.push(Session {
                id: session_id.to_owned(),
                title: title.to_owned(),
                preview: String::new(),
                last_seq: world.cursor,
                turn_active: false,
            });
            true
        }
        EventKind::TurnStarted {
            session_id,
            turn_id,
            user_text,
        } => {
            let preview = truncate(user_text, 60);
            if let Some(session) = session_mut(world, session_id) {
                session.preview = preview;
                session.turn_active = true;
            }
            let chat = chat_mut(world, session_id);
            if chat
                .items
                .iter()
                .any(|item| matches!(item, ChatItem::UserText { turn_id: existing, .. } if existing.as_str() == turn_id))
            {
                return false;
            }
            chat.items.push(ChatItem::UserText {
                turn_id: turn_id.to_owned(),
                text: user_text.to_owned(),
            });
            true
        }
        EventKind::AssistantText {
            session_id,
            turn_id,
            message_id,
            text,
        } => {
            let chat = chat_mut(world, session_id);
            let composite_id = format!("{turn_id}#{message_id}");
            if let Some(ChatItem::AssistantText {
                text: existing, ..
            }) = chat
                .items
                .iter_mut()
                .find(|item| matches!(item, ChatItem::AssistantText { message_id: existing, .. } if existing == &composite_id))
            {
                if *existing == *text {
                    return false;
                }
                *existing = text.to_owned();
                return true;
            }
            chat.items.push(ChatItem::AssistantText {
                turn_id: turn_id.to_owned(),
                message_id: composite_id,
                text: text.to_owned(),
            });
            true
        }
        EventKind::ToolUse {
            session_id,
            turn_id,
            tool_use_id,
            tool_name,
            input_json,
        } => {
            let chat = chat_mut(world, session_id);
            if chat.items.iter().any(|item| {
                matches!(item, ChatItem::ToolUse { tool_use_id: existing, .. } if existing.as_str() == tool_use_id)
            }) {
                return false;
            }
            chat.items.push(ChatItem::ToolUse {
                turn_id: turn_id.to_owned(),
                tool_use_id: tool_use_id.to_owned(),
                tool_name: tool_name.to_owned(),
                input_json: truncate(input_json, 240),
            });
            true
        }
        EventKind::ToolResult {
            session_id,
            turn_id,
            tool_use_id,
            content,
            is_error,
        } => {
            let chat = chat_mut(world, session_id);
            if let Some(ChatItem::ToolResult {
                content: existing, ..
            }) = chat
                .items
                .iter_mut()
                .find(|item| matches!(item, ChatItem::ToolResult { tool_use_id: existing, .. } if existing.as_str() == tool_use_id))
            {
                if *existing == *content {
                    return false;
                }
                *existing = truncate(content, 400);
                return true;
            }
            chat.items.push(ChatItem::ToolResult {
                turn_id: turn_id.to_owned(),
                tool_use_id: tool_use_id.to_owned(),
                content: truncate(content, 400),
                is_error: *is_error,
            });
            true
        }
        EventKind::TurnResult {
            session_id,
            turn_id,
            subtype,
            cost_usd,
            duration_ms,
            ..
        } => {
            if let Some(session) = session_mut(world, session_id) {
                session.turn_active = false;
            }
            let chat = chat_mut(world, session_id);
            if chat.items.iter().any(|item| {
                matches!(item, ChatItem::TurnResult { turn_id: existing, .. } if existing.as_str() == turn_id)
            }) {
                return false;
            }
            chat.items.push(ChatItem::TurnResult {
                turn_id: turn_id.to_owned(),
                subtype: subtype.to_owned(),
                cost_usd: *cost_usd,
                duration_ms: *duration_ms,
            });
            true
        }
        EventKind::PermissionRequested {
            permission_id,
            session_id,
            tool_name,
            input_summary,
            expires_at_ms,
            ..
        } => {
            let chat = chat_mut(world, session_id);
            if let Some(card) = chat.permission.as_ref()
                && card.permission_id == *permission_id
                && card.resolution.is_none()
            {
                return false;
            }
            chat.permission = Some(PermissionCard {
                permission_id: permission_id.to_owned(),
                tool_name: tool_name.to_owned(),
                input_summary: truncate(input_summary, 200),
                expires_at_ms: *expires_at_ms,
                resolution: None,
            });
            true
        }
        EventKind::PermissionResolved {
            permission_id,
            decision,
            resolved_by,
        } => {
            let mut changed = false;
            for chat in world.chats.values_mut() {
                if let Some(card) = chat.permission.as_mut()
                    && card.permission_id == *permission_id
                    && card.resolution.is_none()
                {
                    card.resolution = Some((*decision, *resolved_by));
                    changed = true;
                }
            }
            changed
        }
        EventKind::AgentStatus { online, .. } => {
            if world.agent_online == *online {
                return false;
            }
            world.agent_online = *online;
            true
        }
        EventKind::Notice { text, .. } => {
            let text = truncate(text, 120);
            if world.notices.last().is_some_and(|last| last == &text) {
                return false;
            }
            world.notices.push(text);
            let overflow = world.notices.len().saturating_sub(8);
            world.notices.drain(..overflow);
            true
        }
    }
}

fn session_mut<'a>(world: &'a mut World, session_id: &str) -> Option<&'a mut Session> {
    world
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
}

fn chat_mut<'a>(world: &'a mut World, session_id: &str) -> &'a mut ChatState {
    world.chats.entry(session_id.to_owned()).or_default()
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let mut truncated: String = value.chars().take(max_chars).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_cc_protocol::NoticeLevel;

    fn event(seq: u64, kind: EventKind) -> Event {
        Event {
            seq,
            ts_ms: seq * 1_000,
            kind,
        }
    }

    fn full_turn(seq: u64) -> Vec<Event> {
        vec![
            event(
                seq,
                EventKind::SessionCreated {
                    session_id: "s1".to_owned(),
                    title: Some("tela".to_owned()),
                },
            ),
            event(
                seq + 1,
                EventKind::TurnStarted {
                    session_id: "s1".to_owned(),
                    turn_id: "t1".to_owned(),
                    user_text: "看一下".to_owned(),
                },
            ),
            event(
                seq + 2,
                EventKind::AssistantText {
                    session_id: "s1".to_owned(),
                    turn_id: "t1".to_owned(),
                    message_id: "m1".to_owned(),
                    text: "收到".to_owned(),
                },
            ),
            event(
                seq + 3,
                EventKind::TurnResult {
                    session_id: "s1".to_owned(),
                    turn_id: "t1".to_owned(),
                    subtype: "success".to_owned(),
                    cost_usd: Some(0.01),
                    duration_ms: Some(1_000),
                    session_id_confirmed: None,
                },
            ),
        ]
    }

    #[test]
    fn a_full_turn_builds_session_preview_and_items_in_order() {
        let mut world = World::default();
        for event in full_turn(1) {
            assert!(apply_event(&mut world, &event));
        }
        assert_eq!(world.cursor, 4);
        assert_eq!(world.sessions.len(), 1);
        assert_eq!(world.sessions[0].title, "tela");
        assert_eq!(world.sessions[0].preview, "看一下");
        assert!(!world.sessions[0].turn_active);
        let chat = world.chats.get("s1").expect("chat");
        assert_eq!(chat.items.len(), 3);
        assert!(matches!(&chat.items[0], ChatItem::UserText { text, .. } if text == "看一下"));
        assert!(matches!(
            &chat.items[2],
            ChatItem::TurnResult {
                cost_usd: Some(0.01),
                ..
            }
        ));
    }

    #[test]
    fn duplicate_and_stale_seq_events_are_idempotent() {
        let mut world = World::default();
        for event in full_turn(1) {
            apply_event(&mut world, &event);
        }
        for event in full_turn(1) {
            assert!(!apply_event(&mut world, &event));
        }
        assert_eq!(world.chats["s1"].items.len(), 3);
    }

    #[test]
    fn assistant_text_replaces_by_composite_message_id() {
        let mut world = World::default();
        for event in full_turn(1) {
            apply_event(&mut world, &event);
        }
        assert!(apply_event(
            &mut world,
            &event(
                5,
                EventKind::AssistantText {
                    session_id: "s1".to_owned(),
                    turn_id: "t1".to_owned(),
                    message_id: "m1".to_owned(),
                    text: "收到，这是更新后的完整文本".to_owned(),
                },
            )
        ));
        let chat = &world.chats["s1"];
        let assistant = chat
            .items
            .iter()
            .filter(|item| matches!(item, ChatItem::AssistantText { .. }))
            .count();
        assert_eq!(assistant, 1);
        assert!(
            matches!(&chat.items[1], ChatItem::AssistantText { text, .. } if text == "收到，这是更新后的完整文本")
        );
    }

    #[test]
    fn permission_card_lifecycle_resolves_once() {
        let mut world = World::default();
        for event in full_turn(1) {
            apply_event(&mut world, &event);
        }
        assert!(apply_event(
            &mut world,
            &event(
                5,
                EventKind::PermissionRequested {
                    permission_id: "p1".to_owned(),
                    session_id: "s1".to_owned(),
                    turn_id: "t2".to_owned(),
                    tool_name: "Bash".to_owned(),
                    input_summary: "echo hi".to_owned(),
                    expires_at_ms: 9_999,
                },
            )
        ));
        assert!(apply_event(
            &mut world,
            &event(
                6,
                EventKind::PermissionResolved {
                    permission_id: "p1".to_owned(),
                    decision: PermissionDecision::Allow,
                    resolved_by: PermissionResolver::Phone,
                },
            )
        ));
        let card = world.chats["s1"].permission.as_ref().expect("card");
        assert_eq!(
            card.resolution,
            Some((PermissionDecision::Allow, PermissionResolver::Phone))
        );
        assert!(!apply_event(
            &mut world,
            &event(
                7,
                EventKind::PermissionResolved {
                    permission_id: "p1".to_owned(),
                    decision: PermissionDecision::Deny,
                    resolved_by: PermissionResolver::AgentTimeout,
                },
            )
        ));
        assert_eq!(
            world.chats["s1"].permission.as_ref().unwrap().resolution,
            Some((PermissionDecision::Allow, PermissionResolver::Phone))
        );
    }

    #[test]
    fn agent_status_and_notice_update_the_world() {
        let mut world = World::default();
        assert!(apply_event(
            &mut world,
            &event(
                1,
                EventKind::AgentStatus {
                    online: true,
                    agent_id: "desktop-wsl".to_owned(),
                },
            )
        ));
        assert!(!apply_event(
            &mut world,
            &event(
                2,
                EventKind::AgentStatus {
                    online: true,
                    agent_id: "desktop-wsl".to_owned(),
                },
            )
        ));
        assert!(apply_event(
            &mut world,
            &event(
                3,
                EventKind::Notice {
                    level: NoticeLevel::Error,
                    text: "agent offline".to_owned(),
                },
            )
        ));
        assert_eq!(world.notices, vec!["agent offline".to_owned()]);
    }

    #[test]
    fn long_tool_payloads_are_truncated() {
        let mut world = World::default();
        for event in full_turn(1) {
            apply_event(&mut world, &event);
        }
        let long_input = "x".repeat(500);
        apply_event(
            &mut world,
            &event(
                5,
                EventKind::ToolUse {
                    session_id: "s1".to_owned(),
                    turn_id: "t2".to_owned(),
                    tool_use_id: "call1".to_owned(),
                    tool_name: "Write".to_owned(),
                    input_json: long_input.clone(),
                },
            ),
        );
        let chat = &world.chats["s1"];
        assert!(
            matches!(&chat.items[3], ChatItem::ToolUse { input_json, .. } if input_json.chars().count() == 241)
        );
    }
}
