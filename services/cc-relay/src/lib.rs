//! CC Remote 中继：2 核 2G 服务器上的轻量转发枢纽。
//!
//! 三条线各占一个阻塞线程组：手机 HTTP（每连接一线程）、agent TCP 帧（每连接读写两
//! 线程）、权限清扫（单线程）。共享态全部在 [`RelayState`] 里经 `Mutex`/原子量串行化，
//! 无异步运行时；第三方依赖只有 httparse 与 serde 家族。设计取舍见 docs/038。

pub mod agent_link;
pub mod http;
pub mod persist;
pub mod state;

pub use state::{RelayState, SharedState};

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 中继配置；`main` 从环境变量解析，测试直接构造（用 `127.0.0.1:0` 拿临时口）。
#[derive(Clone, Debug)]
pub struct RelayConfig {
    /// 手机与 agent 共用的 Bearer token。
    pub token: String,
    /// 手机 REST 监听地址。
    pub bind_http: SocketAddr,
    /// agent TCP 帧监听地址。
    pub bind_agent: SocketAddr,
    /// JSONL 持久化目录；`None` 为纯内存（重启丢历史）。
    pub persist_dir: Option<PathBuf>,
}

/// 环境变量键。
pub mod env_keys {
    /// 手机与 agent 共用的 token（必填）。
    pub const TOKEN: &str = "CC_RELAY_TOKEN";
    /// 手机 REST 监听（默认 0.0.0.0:8787）。
    pub const BIND_HTTP: &str = "CC_RELAY_BIND_HTTP";
    /// agent TCP 监听（默认 0.0.0.0:8789）。
    pub const BIND_AGENT: &str = "CC_RELAY_BIND_AGENT";
    /// JSONL 持久化目录（可选）。
    pub const PERSIST_DIR: &str = "CC_RELAY_PERSIST_DIR";
}

impl RelayConfig {
    /// 默认监听：HTTP 8787、agent 8789。
    pub fn defaults(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            bind_http: "0.0.0.0:8787".parse().expect("default HTTP bind"),
            bind_agent: "0.0.0.0:8789".parse().expect("default agent bind"),
            persist_dir: None,
        }
    }

    /// 从环境变量解析；token 缺失或地址畸形时返回错误说明。
    pub fn from_env() -> Result<Self, String> {
        let token = std::env::var(env_keys::TOKEN)
            .map_err(|_| format!("{} is required", env_keys::TOKEN))?;
        let mut config = Self::defaults(token);
        if let Ok(bind) = std::env::var(env_keys::BIND_HTTP) {
            config.bind_http = bind
                .parse()
                .map_err(|_| format!("invalid {}: {bind}", env_keys::BIND_HTTP))?;
        }
        if let Ok(bind) = std::env::var(env_keys::BIND_AGENT) {
            config.bind_agent = bind
                .parse()
                .map_err(|_| format!("invalid {}: {bind}", env_keys::BIND_AGENT))?;
        }
        if let Ok(dir) = std::env::var(env_keys::PERSIST_DIR)
            && !dir.trim().is_empty()
        {
            config.persist_dir = Some(PathBuf::from(dir));
        }
        Ok(config)
    }
}

/// 已启动的中继句柄：真实监听地址 + 停机开关。
pub struct Relay {
    /// 手机 REST 实际监听地址（`:0` 时是分配出的临时口）。
    pub http_addr: SocketAddr,
    /// agent TCP 实际监听地址。
    pub agent_addr: SocketAddr,
    state: SharedState,
    shutdown: Arc<AtomicBool>,
}

impl Relay {
    /// 绑定监听并启动全部线程组。
    pub fn start(config: RelayConfig) -> std::io::Result<Self> {
        let state = match &config.persist_dir {
            Some(dir) => {
                let (handle, replayed) = persist::PersistHandle::open(dir)?;
                RelayState::with_persistence(handle, replayed)
            }
            None => RelayState::in_memory(),
        };
        let state = Arc::new(state);
        let token = Arc::new(config.token);
        let shutdown = Arc::new(AtomicBool::new(false));

        let http_listener = bind(&config.bind_http)?;
        let http_addr = http_listener.local_addr()?;
        let agent_listener = bind(&config.bind_agent)?;
        let agent_addr = agent_listener.local_addr()?;

        std::thread::spawn({
            let state = Arc::clone(&state);
            let token = Arc::clone(&token);
            let shutdown = Arc::clone(&shutdown);
            move || http_accept_loop(http_listener, state, token, shutdown)
        });
        std::thread::spawn({
            let state = Arc::clone(&state);
            let shutdown = Arc::clone(&shutdown);
            move || agent_link::serve_sweeper(state, shutdown)
        });
        std::thread::spawn({
            let state = Arc::clone(&state);
            let token = Arc::clone(&token);
            let shutdown = Arc::clone(&shutdown);
            move || agent_link::serve_agent_link(agent_listener, state, token, shutdown)
        });

        Ok(Self {
            http_addr,
            agent_addr,
            state,
            shutdown,
        })
    }

    /// 共享状态（测试断言用）。
    pub fn state(&self) -> &SharedState {
        &self.state
    }

    /// 请求停机：accept 循环与清扫线程在数百毫秒内退出（存量连接线程随后自然结束）。
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

fn bind(addr: &SocketAddr) -> std::io::Result<TcpListener> {
    let listener = TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

fn http_accept_loop(
    listener: TcpListener,
    state: SharedState,
    token: Arc<String>,
    shutdown: Arc<AtomicBool>,
) {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        match listener.accept() {
            Ok((mut stream, _address)) => {
                let _ = stream.set_nonblocking(false);
                let state = Arc::clone(&state);
                let token = Arc::clone(&token);
                let shutdown = Arc::clone(&shutdown);
                std::thread::spawn(move || {
                    http::handle_connection(&mut stream, &state, &token, &shutdown)
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(500)),
        }
    }
}

/// 以给定配置一直服务（主线程 parked；容器里由 systemd 负责生命周期）。
pub fn serve(config: RelayConfig) -> std::io::Result<()> {
    let relay = Relay::start(config)?;
    println!(
        "tela-cc-relay listening: http={} agent={}",
        relay.http_addr, relay.agent_addr
    );
    loop {
        std::thread::park();
    }
}
