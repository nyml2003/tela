//! 中继事件流：所有端共享的唯一事实时间线。
//!
//! 事件由中继 `append` 时统一赋 `seq`（每用户全局单调、无空洞）与 `ts_ms`；手机端只追
//! 游标，不猜顺序。上行侧（agent）发送的是去掉 `seq`/`ts_ms` 的 [`EventKind`]。

use serde::{Deserialize, Serialize};

/// 权限决策。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

/// 权限请求最终由谁裁决（用于三方竞态的事后审计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionResolver {
    /// 手机先到，裁决生效。
    Phone,
    /// agent 本地超时，默认拒绝。
    AgentTimeout,
    /// 中继清扫线程过期关闭。
    RelayExpired,
}

/// 通知级别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeLevel {
    Info,
    Error,
}

/// 一条已入日志的事件；`kind` 内联展平成同一层 JSON 字段。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// 全局单调事件号（无空洞）。
    pub seq: u64,
    /// 中继 UTC 毫秒时间戳。
    pub ts_ms: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

/// 事件负载；`kind` 字符串即协议的事件类型名。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    /// 会话建立（agent 从 CLI init 消息回填真实 session_id）。
    SessionCreated {
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    /// 一回合开始；`user_text` 供会话列表预览。
    TurnStarted {
        session_id: String,
        turn_id: String,
        user_text: String,
    },
    /// 助手文本（全量替换式；流式增量是 M2 的兼容扩展）。
    AssistantText {
        session_id: String,
        turn_id: String,
        message_id: String,
        text: String,
    },
    /// 助手发起一次工具调用；`input_json` 序列化为字符串以避免异构端结构耦合。
    ToolUse {
        session_id: String,
        turn_id: String,
        tool_use_id: String,
        tool_name: String,
        input_json: String,
    },
    /// 工具结果（或权限拒绝的占位结果）。
    ToolResult {
        session_id: String,
        turn_id: String,
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    /// 一回合结束；`session_id_confirmed` 在 agent 首回合拿到真实 id 后回填。
    TurnResult {
        session_id: String,
        turn_id: String,
        subtype: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost_usd: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id_confirmed: Option<String>,
    },
    /// agent 请求远程批准；手机展示卡片并倒计时。
    PermissionRequested {
        permission_id: String,
        session_id: String,
        turn_id: String,
        tool_name: String,
        input_summary: String,
        expires_at_ms: u64,
    },
    /// 权限请求已被裁决（无论何方）。
    PermissionResolved {
        permission_id: String,
        decision: PermissionDecision,
        resolved_by: PermissionResolver,
    },
    /// 桌面 agent 在线状态变化。
    AgentStatus { online: bool, agent_id: String },
    /// 中继侧诊断通知。
    Notice { level: NoticeLevel, text: String },
}

impl EventKind {
    /// 事件归属的会话；全局事件（agent 状态、通知）返回 `None`。
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::SessionCreated { session_id, .. }
            | Self::TurnStarted { session_id, .. }
            | Self::AssistantText { session_id, .. }
            | Self::ToolUse { session_id, .. }
            | Self::ToolResult { session_id, .. }
            | Self::TurnResult { session_id, .. }
            | Self::PermissionRequested { session_id, .. } => Some(session_id),
            Self::PermissionResolved { .. } | Self::AgentStatus { .. } | Self::Notice { .. } => {
                None
            }
        }
    }

    /// 事件归属的回合；会话级与全局事件返回 `None`。
    pub fn turn_id(&self) -> Option<&str> {
        match self {
            Self::TurnStarted { turn_id, .. }
            | Self::AssistantText { turn_id, .. }
            | Self::ToolUse { turn_id, .. }
            | Self::ToolResult { turn_id, .. }
            | Self::TurnResult { turn_id, .. }
            | Self::PermissionRequested { turn_id, .. } => Some(turn_id),
            Self::SessionCreated { .. }
            | Self::PermissionResolved { .. }
            | Self::AgentStatus { .. }
            | Self::Notice { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_flattens_kind_into_a_single_object() {
        let event = Event {
            seq: 7,
            ts_ms: 1_758_595_200_000,
            kind: EventKind::AssistantText {
                session_id: "s1".to_owned(),
                turn_id: "t1".to_owned(),
                message_id: "m1".to_owned(),
                text: "你好".to_owned(),
            },
        };
        let json = serde_json::to_value(&event).expect("serialize event");
        assert_eq!(json["seq"], 7);
        assert_eq!(json["kind"], "assistant_text");
        assert_eq!(json["text"], "你好");
        let parsed: Event = serde_json::from_value(json).expect("deserialize event");
        assert_eq!(parsed, event);
    }

    #[test]
    fn permission_enums_use_wire_friendly_names() {
        assert_eq!(
            serde_json::to_value(PermissionDecision::Deny).expect("serialize"),
            serde_json::json!("deny")
        );
        assert_eq!(
            serde_json::to_value(PermissionResolver::AgentTimeout).expect("serialize"),
            serde_json::json!("agent-timeout")
        );
    }

    #[test]
    fn session_and_turn_extraction_cover_every_variant() {
        let mut kinds = vec![
            EventKind::SessionCreated {
                session_id: "s".to_owned(),
                title: None,
            },
            EventKind::AgentStatus {
                online: true,
                agent_id: "a".to_owned(),
            },
            EventKind::Notice {
                level: NoticeLevel::Info,
                text: "n".to_owned(),
            },
            EventKind::PermissionResolved {
                permission_id: "p".to_owned(),
                decision: PermissionDecision::Allow,
                resolved_by: PermissionResolver::Phone,
            },
        ];
        for kind in &kinds {
            assert!(kind.turn_id().is_none());
        }
        assert_eq!(kinds[0].session_id(), Some("s"));
        assert!(kinds[1].session_id().is_none());
        kinds.push(EventKind::TurnStarted {
            session_id: "s".to_owned(),
            turn_id: "t".to_owned(),
            user_text: "hi".to_owned(),
        });
        assert_eq!(kinds[4].session_id(), Some("s"));
        assert_eq!(kinds[4].turn_id(), Some("t"));
    }
}
