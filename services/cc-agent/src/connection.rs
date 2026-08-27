//! 到中继的 agent 链路客户端：重连循环 + 长度前缀帧读写。
//!
//! 断线时 `send` 丢弃消息并记日志（事件流可容忍丢失；业务层保持会话子进程，等待重连
//! 后继续上行——v1 已知取舍，见 docs/038）。线程模型：重连线程独占上行收件箱并就地
//! 写帧（心跳 = 空闲 15s 自动补 `Ping`）；每条连接另配一个读线程，断开时置停机标志让
//! 重连线程在下个 500ms 窗口感知。半开连接由心跳写失败暴露。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use tela_cc_protocol::{
    DownlinkMessage, FrameDecoder, PROTOCOL_VERSION, UplinkMessage, decode_frame, encode_frame,
};

use crate::config::AgentConfig;

/// 心跳间隔；中继按 30s 空闲判死，agent 侧发得更勤。
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// 收件箱轮询步长；决定断链感知延迟上限。
const POLL_SLICE: Duration = Duration::from_millis(500);
/// hello 握手等待上限。
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// 读超时：读线程醒来检查停机标志（须大于心跳间隔，正常流量下无感）。
const READ_TIMEOUT: Duration = Duration::from_secs(20);
/// 重连退避：1s 起、翻倍、30s 封顶；连接成功即复位。
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// 到中继的双向通道句柄；后台线程自动重连。
pub struct RelayConnection {
    outgoing: SyncSender<UplinkMessage>,
    commands: Receiver<DownlinkMessage>,
}

impl RelayConnection {
    /// 启动后台重连循环；立即返回句柄。
    pub fn start(config: &AgentConfig) -> Self {
        let (outgoing, inbox) = sync_channel::<UplinkMessage>(1024);
        let (command_out, commands) = sync_channel::<DownlinkMessage>(1024);
        let connection = Self { outgoing, commands };
        let config = config.clone();
        std::thread::Builder::new()
            .name("cc-agent-link".to_owned())
            .spawn(move || run_link(config, inbox, command_out))
            .expect("spawn link thread");
        connection
    }

    /// 发送一条上行消息；断线或队列满时丢弃并记日志。
    pub fn send(&self, message: UplinkMessage) {
        if self.outgoing.try_send(message).is_err() {
            log("上行消息被丢弃（链路断开或队列满）");
        }
    }

    /// 上行通道的发送端克隆（供 [`crate::sessions::SessionManager`] 直接发事件）。
    pub fn uplink_sender(&self) -> SyncSender<UplinkMessage> {
        self.outgoing.clone()
    }

    /// 非阻塞取下一条下行命令。
    pub fn try_next_command(&self) -> Option<DownlinkMessage> {
        self.commands.try_recv().ok()
    }
}

/// 链路主线程：独占收件箱，握手 → 写帧 → 断开 → 退避 → 重连。
fn run_link(
    config: AgentConfig,
    inbox: Receiver<UplinkMessage>,
    command_out: SyncSender<DownlinkMessage>,
) {
    let mut backoff = BACKOFF_MIN;
    loop {
        if let Err(reason) = run_once(&config, &inbox, command_out.clone()) {
            match reason {
                Disconnect::Fatal(reason) => {
                    log(&format!("中继拒绝连接（{reason}），agent 退出"));
                    std::process::exit(1);
                }
                Disconnect::Io => {
                    log(&format!("与中继断开，{}s 后重连", backoff.as_secs()));
                }
            }
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

enum Disconnect {
    Io,
    Fatal(String),
}

/// 一次完整连接生命周期；返回断开原因。
fn run_once(
    config: &AgentConfig,
    inbox: &Receiver<UplinkMessage>,
    command_out: SyncSender<DownlinkMessage>,
) -> Result<(), Disconnect> {
    let mut stream = TcpStream::connect(&config.relay_addr).map_err(|error| {
        log(&format!("连接中继失败: {error}"));
        Disconnect::Io
    })?;
    let _ = stream.set_read_timeout(Some(HELLO_TIMEOUT));
    let hello = UplinkMessage::Hello {
        protocol_version: PROTOCOL_VERSION,
        token: config.token.clone(),
        agent_id: config.agent_id.clone(),
    };
    write_frame(&mut stream, &hello).map_err(|_| Disconnect::Io)?;
    stream.flush().map_err(|_| Disconnect::Io)?;
    match read_hello_ok(&mut stream) {
        HelloResult::Accepted => {}
        HelloResult::Rejected(reason) => return Err(Disconnect::Fatal(reason)),
        HelloResult::Failed => return Err(Disconnect::Io),
    }
    log("已连接中继");

    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let reader_stream = stream.try_clone().map_err(|_| Disconnect::Io)?;
    let stopped = Arc::new(AtomicBool::new(false));
    let reader_stopped = Arc::clone(&stopped);
    let reader = std::thread::Builder::new()
        .name("cc-agent-reader".to_owned())
        .spawn(move || run_reader(reader_stream, reader_stopped, command_out))
        .expect("spawn reader thread");

    // 写循环：独占收件箱；空闲超过心跳间隔补 Ping，停机标志置位即退出。
    let mut last_sent = Instant::now();
    let outcome = loop {
        if stopped.load(Ordering::SeqCst) {
            break Ok(());
        }
        let message = match inbox.recv_timeout(POLL_SLICE) {
            Ok(message) => message,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if last_sent.elapsed() < HEARTBEAT_INTERVAL {
                    continue;
                }
                UplinkMessage::Ping
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // 连接句柄已全部丢弃（进程退出中）；结束链路线程。
                break Ok(());
            }
        };
        if write_frame(&mut stream, &message).is_err() || stream.flush().is_err() {
            break Err(Disconnect::Io);
        }
        last_sent = Instant::now();
    };
    stopped.store(true, Ordering::SeqCst);
    let _ = reader.join();
    outcome
}

fn run_reader(
    mut stream: TcpStream,
    stopped: Arc<AtomicBool>,
    command_out: SyncSender<DownlinkMessage>,
) {
    let mut decoder = FrameDecoder::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        if stopped.load(Ordering::SeqCst) {
            break;
        }
        let read = match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => break,
        };
        if decoder.push(&chunk[..read]).is_err() {
            log("中继帧超限，断开重连");
            break;
        }
        loop {
            match decoder.pop_frame() {
                Ok(Some(payload)) => match decode_frame::<DownlinkMessage>(&payload) {
                    // 中继的 Pong 是对我们心跳的确认；握手确认在此前已消费。
                    Ok(DownlinkMessage::Pong) | Ok(DownlinkMessage::HelloOk { .. }) => {}
                    Ok(message) => {
                        if command_out.try_send(message).is_err() {
                            log("下行命令队列满，丢弃");
                        }
                    }
                    Err(error) => log(&format!("忽略无法解析的中继消息: {error}")),
                },
                Ok(None) => break,
                Err(_) => return,
            }
        }
    }
}

enum HelloResult {
    Accepted,
    Rejected(String),
    Failed,
}

fn read_hello_ok(stream: &mut TcpStream) -> HelloResult {
    let mut decoder = FrameDecoder::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = match stream.read(&mut chunk) {
            Ok(0) => return HelloResult::Failed,
            Ok(read) => read,
            Err(_) => return HelloResult::Failed,
        };
        if decoder.push(&chunk[..read]).is_err() {
            return HelloResult::Failed;
        }
        match decoder.pop_frame() {
            Ok(Some(payload)) => match decode_frame::<DownlinkMessage>(&payload) {
                Ok(DownlinkMessage::HelloOk { .. }) => return HelloResult::Accepted,
                Ok(DownlinkMessage::Error { text }) => return HelloResult::Rejected(text),
                Ok(_) => return HelloResult::Rejected("握手期间收到意外消息".to_owned()),
                Err(_) => return HelloResult::Failed,
            },
            Ok(None) => continue,
            Err(_) => return HelloResult::Failed,
        }
    }
}

fn write_frame(stream: &mut TcpStream, message: &UplinkMessage) -> std::io::Result<()> {
    let frame = encode_frame(message)
        .map_err(|error| std::io::Error::other(format!("encode frame: {error}")))?;
    stream.write_all(&frame)
}

fn log(message: &str) {
    eprintln!("[cc-agent] {message}");
}
