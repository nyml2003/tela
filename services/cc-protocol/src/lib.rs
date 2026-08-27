//! CC Remote 三端共享协议：手机 app、中继（cc-relay）与桌面 agent（cc-agent）的唯一事实源。
//!
//! 职责边界（见 docs/038）：
//! - [`events`]：中继事件流的类型与封装（`seq` 单调、全事件共享一个游标）。
//! - [`wire`]：手机 REST 请求/响应体与 agent 链路的双向消息。
//! - [`bridge`]：宿主桥 `net.http.request` 能力的 postcard 载荷（guest 与 host 共享）。
//! - [`frame`]：agent ↔ 中继 TCP 链路的长度前缀 JSON 帧编解码。
//!
//! 本 crate 刻意零 tela 依赖：中继与 agent 不应感知 UI 基座；手机 app 只消费
//! [`bridge::NetHttpRequest`]/[`bridge::NetHttpResponse`] 与 [`wire::SyncResponse`] 的 JSON 体。

/// 协议版本；不兼容变更时递增并同步两侧校验。
pub const PROTOCOL_VERSION: u32 = 1;

/// 桥 `net.http.request` 单响应 body 上限；超过置 `truncated`，客户端按 cursor 分页续拉。
pub const MAX_RESPONSE_BODY_BYTES: usize = 128 * 1024;

/// 手机 REST 请求 body 上限（超过即 413）。
pub const MAX_REQUEST_BODY_BYTES: usize = 128 * 1024;

/// 权限请求的默认有效期；超时由 agent 侧本地拒绝并上报 `agent-timeout`。
pub const PERMISSION_TIMEOUT_MS: u64 = 120_000;

/// 手机端默认轮询间隔。
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 1_500;

/// 轮询失败时的退避上限。
pub const MAX_POLL_INTERVAL_MS: u64 = 15_000;

/// `GET /v1/sync` 默认与最大分页条数。
pub const SYNC_DEFAULT_LIMIT: usize = 200;
pub const SYNC_MAX_LIMIT: usize = 500;

/// 单条事件的序列化上限；超过即拒绝入日志。
pub const MAX_EVENT_BYTES: usize = 64 * 1024;

/// 中继内存事件日志的环形上限；更早的事件只能从 JSONL 持久层恢复。
pub const EVENT_LOG_CAP: usize = 10_000;

pub mod bridge;
pub mod events;
pub mod frame;
pub mod wire;

pub use bridge::{
    NET_GROUP, NET_NAME, NET_SCOPE, NetHttpMethod, NetHttpRequest, NetHttpResponse,
    decode_net_http_request, decode_net_http_response, encode_net_http_request,
    encode_net_http_response, net_capability_display,
};
pub use events::{Event, EventKind, NoticeLevel, PermissionDecision, PermissionResolver};
pub use frame::{FrameDecoder, FrameError, MAX_FRAME_BYTES, decode_frame, encode_frame};
pub use wire::{
    AcceptedResponse, AgentLinkError, CreateSessionRequest, DownlinkMessage, ErrorBody,
    HealthResponse, PermissionDecisionRequest, PermissionResolvedResponse, SendMessageRequest,
    SyncResponse, UplinkMessage,
};
