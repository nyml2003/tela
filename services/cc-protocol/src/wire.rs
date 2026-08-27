//! 手机 REST 与 agent TCP 链路的线格式。
//!
//! 手机侧是标准 HTTP/1.1 + JSON（Bearer 鉴权，见 docs/038 §协议）；agent 侧是长度前缀
//! JSON 帧（见 [`crate::frame`]），连接后先完成 hello 握手再交换业务消息。

use serde::{Deserialize, Serialize};

use crate::events::{Event, EventKind, PermissionDecision, PermissionResolver};

// ---------------------------------------------------------------------------
// 手机 REST（phone → relay）。
// ---------------------------------------------------------------------------

/// `GET /v1/health` 响应（免鉴权）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub protocol_version: u32,
    pub server_time_ms: u64,
}

/// `POST /v1/sessions` 请求。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    /// 幂等键；中继据此去重手机重试。
    pub client_request_id: String,
}

/// `POST /v1/sessions/<id>/messages` 请求。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub text: String,
    /// 幂等键；回显去重与 agent 侧防重放共用。
    pub client_msg_id: String,
}

/// `POST /v1/permissions/<id>` 请求。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PermissionDecisionRequest {
    pub decision: PermissionDecision,
}

/// 通用受理响应（会话创建、发消息）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcceptedResponse {
    pub accepted: bool,
}

/// `POST /v1/permissions/<id>` 响应；已裁决时返回 409 + [`ErrorBody`]。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PermissionResolvedResponse {
    pub resolved_by: PermissionResolver,
}

/// `GET /v1/sync` 响应。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncResponse {
    /// `seq > since` 的事件，按 `seq` 升序。
    pub events: Vec<Event>,
    /// 本次返回的最大 `seq`（无事件时等于 `since`）。
    pub cursor: u64,
    /// 还有更多事件时应立即再拉一次。
    pub truncated: bool,
    pub agent_online: bool,
    pub server_time_ms: u64,
}

/// 4xx/5xx 的 JSON 错误体。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}

// ---------------------------------------------------------------------------
// agent TCP 链路（双向消息，经 [`crate::frame`] 成帧）。
// ---------------------------------------------------------------------------

/// agent → 中继。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UplinkMessage {
    /// 连接后的第一条帧；校验通过前中继丢弃其余消息。
    Hello {
        protocol_version: u32,
        token: String,
        agent_id: String,
    },
    /// 空闲心跳（双向，30s）。
    Ping,
    /// 上行一条事件负载（`seq`/`ts_ms` 由中继赋值）。
    Event { event: EventKind },
}

/// 中继 → agent。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DownlinkMessage {
    /// hello 握手确认。
    HelloOk {
        protocol_version: u32,
        server_time_ms: u64,
    },
    Pong,
    /// 手机请求新建会话；agent 建立 CLI 子进程后以 `session_created` 回填。
    CreateSession {
        client_request_id: String,
    },
    /// 在指定会话注入一回合用户输入；`resume` 表示需要先 `--resume`。
    RunTurn {
        session_id: String,
        turn_id: String,
        prompt: String,
        resume: bool,
    },
    /// 手机对挂起权限的裁决。
    PermissionDecision {
        permission_id: String,
        decision: PermissionDecision,
    },
    /// 取消一回合（尽力而为；M2 起协议保留）。
    CancelTurn {
        turn_id: String,
    },
    /// 链路层错误（版本不符、鉴权失败等，随后断开）。
    Error {
        text: String,
    },
}

/// agent 链路的应用层错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentLinkError(pub String);

impl std::fmt::Display for AgentLinkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for AgentLinkError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uplink_and_downlink_use_snake_case_type_tags() {
        let up = UplinkMessage::Event {
            event: EventKind::Notice {
                level: crate::events::NoticeLevel::Info,
                text: "hi".to_owned(),
            },
        };
        let json = serde_json::to_value(&up).expect("serialize");
        assert_eq!(json["type"], "event");
        assert_eq!(json["event"]["kind"], "notice");

        let down = DownlinkMessage::RunTurn {
            session_id: "s".to_owned(),
            turn_id: "t".to_owned(),
            prompt: "go".to_owned(),
            resume: true,
        };
        let json = serde_json::to_value(&down).expect("serialize");
        assert_eq!(json["type"], "run_turn");
        assert_eq!(json["resume"], true);
        let parsed: DownlinkMessage = serde_json::from_value(json).expect("deserialize");
        assert_eq!(parsed, down);
    }

    #[test]
    fn sync_response_carries_events_and_cursor() {
        let body = SyncResponse {
            events: vec![Event {
                seq: 3,
                ts_ms: 5,
                kind: EventKind::AgentStatus {
                    online: true,
                    agent_id: "desktop-wsl".to_owned(),
                },
            }],
            cursor: 3,
            truncated: false,
            agent_online: true,
            server_time_ms: 9,
        };
        let json = serde_json::to_string(&body).expect("serialize");
        let parsed: SyncResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, body);
    }
}
