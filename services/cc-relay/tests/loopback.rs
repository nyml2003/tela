//! 环回集成：真监听口 + 真 TcpStream，把手机 REST 与 agent 帧链路串成完整闭环。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use tela_cc_protocol::{
    DownlinkMessage, EventKind, PermissionDecision, PermissionResolver, SyncResponse,
    UplinkMessage, decode_frame, encode_frame,
};
use tela_cc_relay::{Relay, RelayConfig};

const TOKEN: &str = "loopback-token";

fn start_relay() -> Relay {
    let mut config = RelayConfig::defaults(TOKEN);
    config.bind_http = "127.0.0.1:0".parse().expect("http bind");
    config.bind_agent = "127.0.0.1:0".parse().expect("agent bind");
    Relay::start(config).expect("start relay")
}

/// 一次性 HTTP 请求（Connection: close），返回 (状态码, body)。
fn http(relay: &Relay, method: &str, path: &str, body: &[u8], authorized: bool) -> (u16, Vec<u8>) {
    let mut stream = TcpStream::connect(relay.http_addr).expect("connect http");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let auth = if authorized {
        format!("Authorization: Bearer {TOKEN}\r\n")
    } else {
        String::new()
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: loopback\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(request.as_bytes()).expect("write request");
    stream.write_all(body).expect("write body");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).expect("read response");
    let text = String::from_utf8_lossy(&raw);
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("status code");
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.as_bytes().to_vec())
        .unwrap_or_default();
    (status, body)
}

/// 连接 agent 口并完成握手，返回已就绪的流。
fn connect_agent(relay: &Relay, token: &str) -> TcpStream {
    let mut stream = TcpStream::connect(relay.agent_addr).expect("connect agent");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let hello = UplinkMessage::Hello {
        protocol_version: 1,
        token: token.to_owned(),
        agent_id: "loopback-agent".to_owned(),
    };
    stream
        .write_all(&encode_frame(&hello).expect("encode hello"))
        .expect("send hello");
    let message = read_frame(&mut stream).expect("hello_ok frame");
    assert!(matches!(
        decode_frame::<DownlinkMessage>(&message).expect("decode"),
        DownlinkMessage::HelloOk { .. }
    ));
    stream
}

/// 读一条完整帧的载荷。
fn read_frame(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut prefix = [0u8; 4];
    stream.read_exact(&mut prefix).ok()?;
    let length = u32::from_le_bytes(prefix) as usize;
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).ok()?;
    Some(payload)
}

#[test]
fn phone_and_agent_walk_the_full_loop() {
    let relay = start_relay();

    // health 免鉴权；未带 token 的受保护端点 401；无 agent 时建会话 409。
    let (status, body) = http(&relay, "GET", "/v1/health", b"", false);
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("protocol_version"));
    let (status, _) = http(&relay, "GET", "/v1/sync?since=0", b"", false);
    assert_eq!(status, 401);
    let (status, _) = http(
        &relay,
        "POST",
        "/v1/sessions",
        br#"{"client_request_id":"c1"}"#,
        true,
    );
    assert_eq!(status, 409);

    // agent 上线。
    let mut agent = connect_agent(&relay, TOKEN);
    let (status, body) = http(&relay, "GET", "/v1/sync?since=0", b"", true);
    assert_eq!(status, 200);
    let sync: SyncResponse = serde_json::from_slice(&body).expect("parse sync");
    assert!(sync.agent_online);
    assert!(
        sync.events
            .iter()
            .any(|event| matches!(event.kind, EventKind::AgentStatus { online: true, .. }))
    );

    // 手机建会话 → agent 收到 CreateSession；agent 回填 session_created。
    let (status, body) = http(
        &relay,
        "POST",
        "/v1/sessions",
        br#"{"client_request_id":"c2"}"#,
        true,
    );
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&body).contains("accepted"));
    let frame = read_frame(&mut agent).expect("create_session frame");
    assert!(matches!(
        decode_frame::<DownlinkMessage>(&frame).expect("decode"),
        DownlinkMessage::CreateSession { .. }
    ));
    agent
        .write_all(
            &encode_frame(&UplinkMessage::Event {
                event: EventKind::SessionCreated {
                    session_id: "s-loop".to_owned(),
                    title: Some("环回".to_owned()),
                },
            })
            .expect("encode"),
        )
        .expect("uplink session_created");

    // 手机发消息 → 中继落 turn_started 并转发 run_turn。
    let (status, _) = http(
        &relay,
        "POST",
        "/v1/sessions/s-loop/messages",
        r#"{"text":"你好","client_msg_id":"m1"}"#.as_bytes(),
        true,
    );
    assert_eq!(status, 200);
    let frame = read_frame(&mut agent).expect("run_turn frame");
    match decode_frame::<DownlinkMessage>(&frame).expect("decode") {
        DownlinkMessage::RunTurn {
            session_id,
            prompt,
            resume,
            ..
        } => {
            assert_eq!(session_id, "s-loop");
            assert_eq!(prompt, "你好");
            assert!(resume);
        }
        other => panic!("unexpected command {other:?}"),
    }
    let (_, body) = http(&relay, "GET", "/v1/sync?since=0&limit=100", b"", true);
    let sync: SyncResponse = serde_json::from_slice(&body).expect("parse sync");
    assert!(
        sync.events
            .iter()
            .any(|event| matches!(&event.kind, EventKind::TurnStarted { user_text, .. } if user_text == "你好"))
    );

    // 权限闭环：agent 上行 permission_requested → 手机批准 → agent 收到裁决。
    agent
        .write_all(
            &encode_frame(&UplinkMessage::Event {
                event: EventKind::PermissionRequested {
                    permission_id: "perm-loop".to_owned(),
                    session_id: "s-loop".to_owned(),
                    turn_id: "turn-9".to_owned(),
                    tool_name: "Bash".to_owned(),
                    input_summary: "echo hi".to_owned(),
                    // 远期到期（2100 年），测试窗口内不会过期。
                    expires_at_ms: 4_102_444_800_000,
                },
            })
            .expect("encode"),
        )
        .expect("uplink permission_requested");
    let (status, body) = http(
        &relay,
        "POST",
        "/v1/permissions/perm-loop",
        br#"{"decision":"allow"}"#,
        true,
    );
    assert_eq!(status, 200);
    let resolved: tela_cc_protocol::PermissionResolvedResponse =
        serde_json::from_slice(&body).expect("parse resolved");
    assert_eq!(resolved.resolved_by, PermissionResolver::Phone);
    let frame = read_frame(&mut agent).expect("permission decision frame");
    assert!(matches!(
        decode_frame::<DownlinkMessage>(&frame).expect("decode"),
        DownlinkMessage::PermissionDecision {
            decision: PermissionDecision::Allow,
            ..
        }
    ));
    let (_, body) = http(&relay, "GET", "/v1/sync?since=0&limit=100", b"", true);
    let sync: SyncResponse = serde_json::from_slice(&body).expect("parse sync");
    assert!(
        sync.events
            .iter()
            .any(|event| matches!(&event.kind, EventKind::PermissionResolved { resolved_by, .. } if *resolved_by == PermissionResolver::Phone))
    );

    relay.stop();
}

#[test]
fn wrong_token_handshake_is_rejected() {
    let relay = start_relay();
    let mut stream = TcpStream::connect(relay.agent_addr).expect("connect agent");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let hello = UplinkMessage::Hello {
        protocol_version: 1,
        token: "wrong".to_owned(),
        agent_id: "intruder".to_owned(),
    };
    stream
        .write_all(&encode_frame(&hello).expect("encode"))
        .expect("send hello");
    let frame = read_frame(&mut stream).expect("error frame");
    assert!(matches!(
        decode_frame::<DownlinkMessage>(&frame).expect("decode"),
        DownlinkMessage::Error { .. }
    ));
    relay.stop();
}
