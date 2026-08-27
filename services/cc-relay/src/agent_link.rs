//! agent TCP 链路（默认 8789）：长度前缀 JSON 帧 + hello 握手。
//!
//! 连接生命周期：10s 内等 `hello`（版本 + token 校验）→ 回 `hello_ok` 并登记 `AgentHub`
//! → 读循环（`event` 入日志、`ping` 回 `pong`）→ 断开时摘除并广播 `agent_status`。命令下发
//! 走每连接一条 mpsc + 写线程；清扫线程周期把过期未决权限判为 relay-expired。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use tela_cc_protocol::{
    DownlinkMessage, FrameDecoder, PROTOCOL_VERSION, UplinkMessage, decode_frame, encode_frame,
};

use crate::state::{SharedState, now_ms};

/// 等待 hello 的窗口；超时即断开。
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

/// 空闲上限：agent 每 30s 心跳，两个周期无流量视作死亡。
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// 清扫周期。
const SWEEP_INTERVAL: Duration = Duration::from_secs(5);

/// 读块大小。
const READ_CHUNK: usize = 8 * 1024;

/// agent 监听主循环（阻塞；`shutdown` 置位后退出）。
pub(crate) fn serve_agent_link(
    listener: TcpListener,
    state: SharedState,
    token: Arc<String>,
    shutdown: Arc<AtomicBool>,
) {
    let _ = listener.set_nonblocking(true);
    loop {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        match listener.accept() {
            Ok((stream, _address)) => {
                // 监听器是非阻塞的；连接恢复阻塞 + 超时驱动的读循环。
                let _ = stream.set_nonblocking(false);
                let state = Arc::clone(&state);
                let token = Arc::clone(&token);
                let shutdown = Arc::clone(&shutdown);
                std::thread::spawn(move || handle_agent(stream, state, token, shutdown));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    }
}

/// 清扫循环（阻塞；`shutdown` 置位后退出）。
pub(crate) fn serve_sweeper(state: SharedState, shutdown: Arc<AtomicBool>) {
    loop {
        for _ in 0..(SWEEP_INTERVAL.as_millis() / 100) {
            if shutdown.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        state.expire_permissions();
    }
}

fn handle_agent(
    mut stream: TcpStream,
    state: SharedState,
    token: Arc<String>,
    shutdown: Arc<AtomicBool>,
) {
    let _ = stream.set_read_timeout(Some(HELLO_TIMEOUT));
    let peer = stream
        .peer_addr()
        .map(|address| address.to_string())
        .unwrap_or_default();
    let mut decoder = FrameDecoder::new();
    let hello = match read_message(&mut stream, &mut decoder) {
        Some(UplinkMessage::Hello {
            protocol_version,
            token: presented,
            agent_id,
        }) => {
            if protocol_version != PROTOCOL_VERSION || presented != *token {
                let _ = write_message(
                    &mut stream,
                    &DownlinkMessage::Error {
                        text: "handshake rejected: version or token mismatch".to_owned(),
                    },
                );
                eprintln!("tela-cc-relay: agent handshake rejected ({peer})");
                return;
            }
            (protocol_version, agent_id)
        }
        Some(_) => {
            let _ = write_message(
                &mut stream,
                &DownlinkMessage::Error {
                    text: "expected hello as the first frame".to_owned(),
                },
            );
            return;
        }
        None => return,
    };

    let (tx, rx) = mpsc::channel::<DownlinkMessage>();
    // 写线程：消费命令通道；发送 Bye 让对端感知关停。
    let writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(_) => return,
    };
    std::thread::spawn(move || {
        let mut writer = writer;
        for message in rx {
            if write_message(&mut writer, &message).is_err() {
                break;
            }
        }
    });

    let connection_id = state.mark_agent_online(&hello.1, tx.clone());
    let _ = tx.send(DownlinkMessage::HelloOk {
        protocol_version: hello.0,
        server_time_ms: now_ms(),
    });
    eprintln!("tela-cc-relay: agent '{}' online ({peer})", hello.1);

    // 读循环：放宽超时到空闲上限。
    let _ = stream.set_read_timeout(Some(IDLE_TIMEOUT));
    let mut chunk = vec![0; READ_CHUNK];
    'read: loop {
        if shutdown.load(Ordering::Relaxed) {
            break 'read;
        }
        match stream.read(&mut chunk) {
            Ok(0) => break 'read,
            Ok(read) => {
                if decoder.push(&chunk[..read]).is_err() {
                    eprintln!("tela-cc-relay: agent {peer} sent an oversized frame");
                    break 'read;
                }
                while let Ok(Some(payload)) = decoder.pop_frame() {
                    match decode_frame::<UplinkMessage>(&payload) {
                        Ok(UplinkMessage::Ping) => {
                            if tx.send(DownlinkMessage::Pong).is_err() {
                                break 'read;
                            }
                        }
                        Ok(UplinkMessage::Event { event }) => {
                            if let Err(error) = state.ingest_event(event) {
                                eprintln!("tela-cc-relay: agent event rejected: {error}");
                            }
                        }
                        Ok(UplinkMessage::Hello { .. }) => {
                            let _ = tx.send(DownlinkMessage::Error {
                                text: "hello is only valid as the first frame".to_owned(),
                            });
                        }
                        Err(error) => {
                            eprintln!("tela-cc-relay: bad frame from agent {peer}: {error}");
                            break 'read;
                        }
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(_) => break 'read,
        }
    }

    state.mark_agent_offline(connection_id);
    eprintln!("tela-cc-relay: agent '{}' offline ({peer})", hello.1);
}

/// 读到下一条完整消息；超时、断连或畸形帧返回 `None`。
fn read_message(stream: &mut TcpStream, decoder: &mut FrameDecoder) -> Option<UplinkMessage> {
    let mut chunk = vec![0; READ_CHUNK];
    loop {
        if let Ok(Some(payload)) = decoder.pop_frame() {
            return decode_frame(&payload).ok();
        }
        match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(read) => decoder.push(&chunk[..read]).ok()?,
            Err(_) => return None,
        }
    }
}

fn write_message(stream: &mut TcpStream, message: &DownlinkMessage) -> std::io::Result<()> {
    let frame = encode_frame(message)
        .map_err(|error| std::io::Error::other(format!("encode downlink frame: {error}")))?;
    stream.write_all(&frame)?;
    stream.flush()
}
