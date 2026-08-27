//! agent 配置：环境变量 + 命令行开关。

use std::path::PathBuf;

/// agent 运行配置。
#[derive(Clone, Debug, PartialEq)]
pub struct AgentConfig {
    /// 中继 agent 链路地址（host:port）。
    pub relay_addr: String,
    /// 中继共享 token。
    pub token: String,
    /// 本 agent 的展示标识。
    pub agent_id: String,
    /// 允许会话使用的工作目录白名单；空表示拒绝一切 `run_turn`。
    pub cwds: Vec<PathBuf>,
    /// claude CLI 可执行文件。
    pub claude: String,
    /// fake 模式：不起子进程，用内置剧本驱动全链路联调。
    pub fake: bool,
}

/// 配置缺失或非法。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

impl AgentConfig {
    /// 从环境变量与命令行参数构造。
    ///
    /// - `CC_AGENT_RELAY_ADDR`（必填）、`CC_AGENT_TOKEN`（必填）
    /// - `CC_AGENT_ID`（默认 `desktop-wsl`）
    /// - `CC_AGENT_CWDS`（冒号分隔的目录白名单）
    /// - `CC_AGENT_CLAUDE`（默认 `claude`）
    /// - `CC_AGENT_FAKE=1` 或 `--fake` 开关
    pub fn from_env_and_args(args: &[String]) -> Result<Self, ConfigError> {
        let fake = args.iter().any(|arg| arg == "--fake")
            || std::env::var("CC_AGENT_FAKE").ok().as_deref() == Some("1");
        let relay_addr = required_env("CC_AGENT_RELAY_ADDR")?;
        let token = required_env("CC_AGENT_TOKEN")?;
        let agent_id = std::env::var("CC_AGENT_ID").unwrap_or_else(|_| "desktop-wsl".to_owned());
        let cwds = std::env::var("CC_AGENT_CWDS")
            .unwrap_or_default()
            .split(':')
            .filter(|entry| !entry.is_empty())
            .map(PathBuf::from)
            .collect();
        let claude = std::env::var("CC_AGENT_CLAUDE").unwrap_or_else(|_| "claude".to_owned());
        Ok(Self {
            relay_addr,
            token,
            agent_id,
            cwds,
            claude,
            fake,
        })
    }

    /// 会话使用的默认工作目录；白名单为空时返回 `None`（拒绝起进程）。
    ///
    /// v1 简化：所有会话共用第一个白名单目录；按会话选择目录是 M1 扩展。
    pub fn default_cwd(&self) -> Option<&std::path::Path> {
        self.cwds.first().map(std::path::PathBuf::as_path)
    }
}

fn required_env(name: &str) -> Result<String, ConfigError> {
    std::env::var(name).map_err(|_| ConfigError(format!("缺少环境变量 {name}")))
}
