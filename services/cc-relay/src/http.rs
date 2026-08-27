//! 手机 REST 口：手搓 HTTP/1.1（httparse 解析），Bearer 鉴权，keep-alive。
//!
//! 刻意只支持自家客户端（ureq/curl）需要的子集：Content-Length 定长 body、无 chunked、
//! 无压缩。畸形请求直接断连——这是一个带 token 才能进入的私有 API，不做通用 Web 服务器
//! 的容错义务。路由是纯函数（吃解析产物、吐状态码 + JSON），单测无需起网络。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tela_cc_protocol::{
    AcceptedResponse, CreateSessionRequest, DownlinkMessage, ErrorBody, EventKind, HealthResponse,
    MAX_REQUEST_BODY_BYTES, PROTOCOL_VERSION, PermissionDecisionRequest,
    PermissionResolvedResponse, SYNC_DEFAULT_LIMIT, SYNC_MAX_LIMIT, SendMessageRequest,
    SyncResponse,
};

use crate::state::{DecideOutcome, RelayState, now_ms};

/// 请求头（含请求行）的读取上限；超过按畸形处理。
const HEADER_LIMIT: usize = 16 * 1024;

/// 每连接读块；body 在 [`MAX_REQUEST_BODY_BYTES`] 内另行累积。
const READ_CHUNK: usize = 8 * 1024;

/// 手机连接的空闲上限；超时即关闭（下次轮询重连）。
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// 解析后的请求：路由所需的全部信息（含鉴权头与 keep-alive 意愿）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedRequest {
    pub method: String,
    /// 不含 query 的路径（已做百分号解码）。
    pub path: String,
    pub query: HashMap<String, String>,
    pub body: Vec<u8>,
    /// `Authorization` 头原值（如有）。
    pub authorization: Option<String>,
    /// 对端要求 `Connection: close`。
    pub connection_close: bool,
    /// 声明了 `Transfer-Encoding: chunked`；路由层统一回 501。
    pub chunked: bool,
}

/// 路由产物：状态码 + JSON body。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    fn json(status: u16, value: impl serde::Serialize) -> Self {
        Self {
            status,
            body: serde_json::to_vec(&value).expect("serialize HTTP response"),
        }
    }

    fn error(status: u16, message: &str) -> Self {
        Self::json(
            status,
            ErrorBody {
                error: message.to_owned(),
            },
        )
    }
}

/// 服务一个手机连接：keep-alive 循环，直到对端关闭、空闲超时、畸形或停机。
pub(crate) fn handle_connection(
    stream: &mut TcpStream,
    state: &Arc<RelayState>,
    token: &str,
    shutdown: &AtomicBool,
) {
    let _ = stream.set_read_timeout(Some(IDLE_TIMEOUT));
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = vec![0; READ_CHUNK];
    loop {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        match try_parse_request(&pending) {
            Parsed::Request(request, consumed) => {
                let keep_alive = !request.connection_close;
                let response = route(state, token, request);
                if write_response(stream, &response, keep_alive).is_err() || !keep_alive {
                    return;
                }
                pending.drain(..consumed);
            }
            Parsed::Malformed => {
                let _ = write_response(
                    stream,
                    &HttpResponse::error(400, "malformed-request"),
                    false,
                );
                return;
            }
            Parsed::TooLarge => {
                let _ = write_response(
                    stream,
                    &HttpResponse::error(413, "payload-too-large"),
                    false,
                );
                return;
            }
            Parsed::Incomplete => {}
        }
        match stream.read(&mut chunk) {
            Ok(0) => return, // 对端关闭
            Ok(read) => {
                if pending.len() + read > HEADER_LIMIT + MAX_REQUEST_BODY_BYTES {
                    let _ = write_response(
                        stream,
                        &HttpResponse::error(413, "payload-too-large"),
                        false,
                    );
                    return;
                }
                pending.extend_from_slice(&chunk[..read]);
            }
            // 空闲超时（含半截请求挂死）：关闭，手机短轮询会重连重发。
            Err(_) => return,
        }
    }
}

/// 一次解析尝试的结果。
#[derive(Debug)]
enum Parsed {
    /// 完整请求 + 消耗的字节数（头 + body）。
    Request(ParsedRequest, usize),
    /// 字节尚不完整。
    Incomplete,
    Malformed,
    TooLarge,
}

fn try_parse_request(buffer: &[u8]) -> Parsed {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut parser = httparse::Request::new(&mut headers);
    let head_len = match parser.parse(buffer) {
        Ok(httparse::Status::Complete(head_len)) => head_len,
        Ok(httparse::Status::Partial) => {
            if buffer.len() > HEADER_LIMIT {
                return Parsed::Malformed;
            }
            return Parsed::Incomplete;
        }
        Err(_) => return Parsed::Malformed,
    };
    if head_len > HEADER_LIMIT {
        return Parsed::Malformed;
    }
    let mut content_length = 0usize;
    let mut authorization = None;
    let mut connection_close = false;
    let mut chunked = false;
    for header in parser.headers.iter() {
        let value = String::from_utf8_lossy(header.value).trim().to_owned();
        match header.name.to_ascii_lowercase().as_str() {
            "content-length" => match value.parse::<usize>() {
                Ok(length) => content_length = length,
                Err(_) => return Parsed::Malformed,
            },
            "authorization" => authorization = Some(value),
            "connection" => {
                connection_close = value.to_ascii_lowercase().contains("close");
            }
            "transfer-encoding" => {
                chunked = value.to_ascii_lowercase().contains("chunked");
            }
            _ => {}
        }
    }
    // chunked 请求没有可预知的长度，无法继续读 body；解析出头后交给路由统一回 501。
    if chunked {
        content_length = 0;
    } else if content_length > MAX_REQUEST_BODY_BYTES {
        return Parsed::TooLarge;
    }
    if buffer.len() < head_len + content_length {
        return Parsed::Incomplete;
    }
    let (path, query) = split_query(parser.path.unwrap_or_default());
    Parsed::Request(
        ParsedRequest {
            method: parser.method.unwrap_or_default().to_owned(),
            path,
            query,
            body: buffer[head_len..head_len + content_length].to_vec(),
            authorization,
            connection_close,
            chunked,
        },
        head_len + content_length,
    )
}

fn split_query(raw: &str) -> (String, HashMap<String, String>) {
    let (path, query) = match raw.split_once('?') {
        Some((path, query)) => (path, query),
        None => (raw, ""),
    };
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(url_decode(key), url_decode(value));
    }
    (url_decode(path), map)
}

/// 极小百分号解码（只服务自家客户端发出的简单值）。
fn url_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                .ok()
                .and_then(|slice| u8::from_str_radix(slice, 16).ok());
            if let Some(byte) = hex {
                decoded.push(byte);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn write_response(
    stream: &mut TcpStream,
    response: &HttpResponse,
    keep_alive: bool,
) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        501 => "Not Implemented",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: {}\r\n\r\n",
        response.status,
        reason,
        response.body.len(),
        if keep_alive { "keep-alive" } else { "close" }
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

// ---------------------------------------------------------------------------
// 路由（纯函数；单测直接构造 RelayState 调用）。
// ---------------------------------------------------------------------------

/// 鉴权 + 分发。
pub fn route(state: &RelayState, token: &str, request: ParsedRequest) -> HttpResponse {
    if request.path == "/v1/health" && request.method == "GET" {
        return HttpResponse::json(
            200,
            HealthResponse {
                protocol_version: PROTOCOL_VERSION,
                server_time_ms: now_ms(),
            },
        );
    }
    if request.chunked {
        return HttpResponse::error(501, "chunked-encoding-not-supported");
    }
    let expected = format!("Bearer {token}");
    if request.authorization.as_deref() != Some(expected.as_str()) {
        return HttpResponse::error(401, "unauthorized");
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/v1/sessions") => create_session(state, &request.body),
        ("GET", "/v1/sync") => sync(state, &request.query),
        ("POST", path) if path.starts_with("/v1/sessions/") && path.ends_with("/messages") => {
            let session_id = &path["/v1/sessions/".len()..path.len() - "/messages".len()];
            send_message(state, session_id, &request.body)
        }
        ("POST", path) if path.starts_with("/v1/permissions/") => {
            let permission_id = &path["/v1/permissions/".len()..];
            decide_permission(state, permission_id, &request.body)
        }
        _ => HttpResponse::error(404, "not-found"),
    }
}

fn create_session(state: &RelayState, body: &[u8]) -> HttpResponse {
    let request: CreateSessionRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(_) => return HttpResponse::error(422, "invalid-body"),
    };
    if state.seen_requests.mark(&request.client_request_id) {
        return HttpResponse::json(200, AcceptedResponse { accepted: true });
    }
    if state
        .agents
        .send(DownlinkMessage::CreateSession {
            client_request_id: request.client_request_id,
        })
        .is_err()
    {
        return HttpResponse::error(409, "agent-offline");
    }
    HttpResponse::json(200, AcceptedResponse { accepted: true })
}

fn sync(state: &RelayState, query: &HashMap<String, String>) -> HttpResponse {
    let since: u64 = query
        .get("since")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let limit = query
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(SYNC_DEFAULT_LIMIT)
        .min(SYNC_MAX_LIMIT);
    if since > state.log.latest_seq() {
        return HttpResponse::error(409, "cursor_reset");
    }
    let (events, truncated) = state.log.since(since, limit);
    let cursor = events.last().map_or(since, |event| event.seq);
    HttpResponse::json(
        200,
        SyncResponse {
            events,
            cursor,
            truncated,
            agent_online: state.agents.online(),
            server_time_ms: now_ms(),
        },
    )
}

fn send_message(state: &RelayState, session_id: &str, body: &[u8]) -> HttpResponse {
    let request: SendMessageRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(_) => return HttpResponse::error(422, "invalid-body"),
    };
    if !state.sessions.contains(session_id) {
        return HttpResponse::error(404, "unknown-session");
    }
    if state.seen_requests.mark(&request.client_msg_id) {
        return HttpResponse::json(200, AcceptedResponse { accepted: true });
    }
    let turn_id = format!("turn-{}", state.log.peek_next_seq());
    if state
        .append_event(EventKind::TurnStarted {
            session_id: session_id.to_owned(),
            turn_id: turn_id.clone(),
            user_text: request.text.clone(),
        })
        .is_err()
    {
        return HttpResponse::error(500, "event-rejected");
    }
    if state
        .agents
        .send(DownlinkMessage::RunTurn {
            session_id: session_id.to_owned(),
            turn_id,
            prompt: request.text,
            resume: true,
        })
        .is_err()
    {
        return HttpResponse::error(409, "agent-offline");
    }
    HttpResponse::json(200, AcceptedResponse { accepted: true })
}

fn decide_permission(state: &RelayState, permission_id: &str, body: &[u8]) -> HttpResponse {
    let request: PermissionDecisionRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(_) => return HttpResponse::error(422, "invalid-body"),
    };
    match state
        .permissions
        .decide(permission_id, request.decision, now_ms())
    {
        Ok(DecideOutcome::Resolved(decision, resolved_by)) => {
            let _ = state.append_event(EventKind::PermissionResolved {
                permission_id: permission_id.to_owned(),
                decision,
                resolved_by,
            });
            let _ = state.agents.send(DownlinkMessage::PermissionDecision {
                permission_id: permission_id.to_owned(),
                decision,
            });
            HttpResponse::json(200, PermissionResolvedResponse { resolved_by })
        }
        Ok(DecideOutcome::AlreadyResolved(_, resolved_by)) => {
            let serialized = serde_json::to_value(resolved_by)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "unknown".to_owned());
            HttpResponse::error(409, &format!("already-resolved:{serialized}"))
        }
        Err(_) => HttpResponse::error(404, "unknown-permission"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use tela_cc_protocol::{EventKind, PermissionResolver};

    const TOKEN: &str = "test-token";

    fn authorized_request(method: &str, path: &str, body: &[u8]) -> ParsedRequest {
        ParsedRequest {
            method: method.to_owned(),
            path: path.to_owned(),
            query: HashMap::new(),
            body: body.to_owned(),
            authorization: Some(format!("Bearer {TOKEN}")),
            connection_close: false,
            chunked: false,
        }
    }

    fn state_with_agent() -> (RelayState, mpsc::Receiver<DownlinkMessage>) {
        let state = RelayState::in_memory();
        let (tx, rx) = mpsc::channel();
        state.mark_agent_online("desktop-test", tx);
        (state, rx)
    }

    #[test]
    fn health_is_public_and_reports_protocol_version() {
        let state = RelayState::in_memory();
        let response = route(
            &state,
            TOKEN,
            ParsedRequest {
                method: "GET".to_owned(),
                path: "/v1/health".to_owned(),
                ..ParsedRequest::default()
            },
        );
        assert_eq!(response.status, 200);
        let health: HealthResponse = serde_json::from_slice(&response.body).expect("parse");
        assert_eq!(health.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn missing_or_wrong_token_is_unauthorized() {
        let state = RelayState::in_memory();
        let mut request = authorized_request("GET", "/v1/sync", b"");
        request.authorization = None;
        assert_eq!(route(&state, TOKEN, request).status, 401);
        let mut wrong = authorized_request("GET", "/v1/sync", b"");
        wrong.authorization = Some("Bearer nope".to_owned());
        assert_eq!(route(&state, TOKEN, wrong).status, 401);
    }

    #[test]
    fn chunked_requests_get_501_even_before_auth() {
        let state = RelayState::in_memory();
        let mut request = authorized_request("POST", "/v1/sessions", b"");
        request.chunked = true;
        request.authorization = None;
        let response = route(&state, TOKEN, request);
        assert_eq!(response.status, 501);
    }

    #[test]
    fn create_session_requires_online_agent_and_dedups() {
        let offline = RelayState::in_memory();
        let body = br#"{"client_request_id":"c1"}"#;
        assert_eq!(
            route(
                &offline,
                TOKEN,
                authorized_request("POST", "/v1/sessions", body)
            )
            .status,
            409
        );

        let (state, rx) = state_with_agent();
        let response = route(
            &state,
            TOKEN,
            authorized_request("POST", "/v1/sessions", body),
        );
        assert_eq!(response.status, 200);
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(1)).expect("command"),
            DownlinkMessage::CreateSession { .. }
        ));
        // 同一 client_request_id 重试：不再下发。
        let retry = route(
            &state,
            TOKEN,
            authorized_request("POST", "/v1/sessions", body),
        );
        assert_eq!(retry.status, 200);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn sync_reports_cursor_reset_and_pages() {
        let (state, _rx) = state_with_agent();
        for index in 0..4 {
            state
                .append_event(EventKind::Notice {
                    level: tela_cc_protocol::NoticeLevel::Info,
                    text: format!("n{index}"),
                })
                .expect("append");
        }
        let mut query = HashMap::new();
        query.insert("since".to_owned(), "1".to_owned());
        query.insert("limit".to_owned(), "2".to_owned());
        let response = route(
            &state,
            TOKEN,
            ParsedRequest {
                method: "GET".to_owned(),
                path: "/v1/sync".to_owned(),
                query,
                authorization: Some(format!("Bearer {TOKEN}")),
                ..ParsedRequest::default()
            },
        );
        assert_eq!(response.status, 200);
        let sync: SyncResponse = serde_json::from_slice(&response.body).expect("parse");
        assert_eq!(sync.events.len(), 2);
        assert!(sync.truncated);
        assert_eq!(sync.cursor, 3);
        assert!(sync.agent_online);

        let mut ahead = HashMap::new();
        ahead.insert("since".to_owned(), "99".to_owned());
        let response = route(
            &state,
            TOKEN,
            ParsedRequest {
                method: "GET".to_owned(),
                path: "/v1/sync".to_owned(),
                query: ahead,
                authorization: Some(format!("Bearer {TOKEN}")),
                ..ParsedRequest::default()
            },
        );
        assert_eq!(response.status, 409);
    }

    #[test]
    fn send_message_appends_turn_started_and_forwards_run_turn() {
        let (state, rx) = state_with_agent();
        let _ = state.ingest_event(EventKind::SessionCreated {
            session_id: "s1".to_owned(),
            title: Some("测试".to_owned()),
        });
        let body = r#"{"text":"你好","client_msg_id":"m1"}"#.as_bytes();
        let response = route(
            &state,
            TOKEN,
            authorized_request("POST", "/v1/sessions/s1/messages", body),
        );
        assert_eq!(response.status, 200);
        let command = rx.recv_timeout(Duration::from_secs(1)).expect("command");
        match command {
            DownlinkMessage::RunTurn {
                session_id,
                turn_id,
                prompt,
                resume,
            } => {
                assert_eq!(session_id, "s1");
                assert_eq!(prompt, "你好");
                assert!(resume);
                assert!(turn_id.starts_with("turn-"));
            }
            other => panic!("unexpected command {other:?}"),
        }
        let (events, _) = state.log.since(0, 10);
        assert!(events.iter().any(|event| matches!(
            &event.kind,
            EventKind::TurnStarted { user_text, .. } if user_text == "你好"
        )));
        // 未知会话 → 404。
        assert_eq!(
            route(
                &state,
                TOKEN,
                authorized_request("POST", "/v1/sessions/ghost/messages", body)
            )
            .status,
            404
        );
    }

    #[test]
    fn permission_decide_resolves_once_and_notifies_agent() {
        let (state, rx) = state_with_agent();
        let _ = state.ingest_event(EventKind::PermissionRequested {
            permission_id: "perm-1".to_owned(),
            session_id: "s1".to_owned(),
            turn_id: "turn-1".to_owned(),
            tool_name: "Bash".to_owned(),
            input_summary: "echo hi".to_owned(),
            expires_at_ms: now_ms() + 60_000,
        });
        let body = br#"{"decision":"allow"}"#;
        let response = route(
            &state,
            TOKEN,
            authorized_request("POST", "/v1/permissions/perm-1", body),
        );
        assert_eq!(response.status, 200);
        let resolved: PermissionResolvedResponse =
            serde_json::from_slice(&response.body).expect("parse");
        assert_eq!(resolved.resolved_by, PermissionResolver::Phone);
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(1)).expect("command"),
            DownlinkMessage::PermissionDecision { .. }
        ));
        // 重复裁决 → 409。
        assert_eq!(
            route(
                &state,
                TOKEN,
                authorized_request("POST", "/v1/permissions/perm-1", body)
            )
            .status,
            409
        );
        // 未知权限 → 404。
        assert_eq!(
            route(
                &state,
                TOKEN,
                authorized_request("POST", "/v1/permissions/ghost", body)
            )
            .status,
            404
        );
    }

    #[test]
    fn invalid_bodies_are_422_and_unknown_routes_404() {
        let (state, _rx) = state_with_agent();
        assert_eq!(
            route(
                &state,
                TOKEN,
                authorized_request("POST", "/v1/sessions", b"not json")
            )
            .status,
            422
        );
        assert_eq!(
            route(&state, TOKEN, authorized_request("GET", "/v1/nope", b"")).status,
            404
        );
    }

    #[test]
    fn url_decoding_handles_paths_and_queries() {
        let (path, query) = split_query("/v1/sync?since=0&text=%E4%BD%A0%E5%A5%BD");
        assert_eq!(path, "/v1/sync");
        assert_eq!(query.get("text").map(String::as_str), Some("你好"));
        assert_eq!(query.get("since").map(String::as_str), Some("0"));
    }

    #[test]
    fn parser_reads_headers_body_and_keep_alive() {
        let raw = b"POST /v1/sessions HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer t\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello_extra";
        match try_parse_request(raw) {
            Parsed::Request(request, consumed) => {
                assert_eq!(request.method, "POST");
                assert_eq!(request.path, "/v1/sessions");
                assert_eq!(request.body, b"hello");
                assert!(request.connection_close);
                assert_eq!(request.authorization.as_deref(), Some("Bearer t"));
                assert_eq!(consumed, raw.len() - "_extra".len());
            }
            other => panic!("unexpected parse {other:?}"),
        }
        // 半截请求 → Incomplete。
        assert!(matches!(
            try_parse_request(b"POST /v1/sessions HTTP/1.1\r\nContent-Length: 5\r\n\r\nhe"),
            Parsed::Incomplete
        ));
        // 畸形 → Malformed。
        assert!(matches!(
            try_parse_request(b"garbage\r\n\r\n"),
            Parsed::Malformed
        ));
    }
}
