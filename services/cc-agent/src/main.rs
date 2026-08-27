//! agent 二进制入口：装配配置、链路与会话管理（或 fake 剧本）并阻塞分发。
//!
//! 生命周期说明：v1 不装信号处理器——agent 被 kill 时 `ChildStdin` 随进程关闭，claude
//! 子进程在 stdin EOF 后自然退出，无需显式清理；Ctrl-C 场景同理（M1 再评估信号方案）。

mod claude;
mod config;
mod connection;
mod fake;
mod sessions;

use std::time::Duration;

use tela_cc_protocol::UplinkMessage;

use config::AgentConfig;
use connection::RelayConnection;
use fake::FakeAgent;
use sessions::SessionManager;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match AgentConfig::from_env_and_args(&args) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[cc-agent] 配置错误: {error}");
            std::process::exit(2);
        }
    };
    eprintln!(
        "[cc-agent] 启动：relay={} id={} fake={}",
        config.relay_addr, config.agent_id, config.fake
    );
    let connection = RelayConnection::start(&config);

    let mut fake = config.fake.then(FakeAgent::new);
    let manager =
        (!config.fake).then(|| SessionManager::new(config.clone(), connection.uplink_sender()));

    loop {
        while let Some(command) = connection.try_next_command() {
            match (&mut fake, &manager) {
                (Some(fake), None) => {
                    let now = sessions::now_ms();
                    for event in fake.handle_command(&command, now) {
                        connection.send(UplinkMessage::Event { event });
                    }
                }
                (None, Some(manager)) => manager.handle_command(command),
                _ => unreachable!("fake 与真实管理器互斥"),
            }
        }
        let now = sessions::now_ms();
        match (&mut fake, &manager) {
            (Some(fake), None) => {
                for event in fake.tick(now) {
                    connection.send(UplinkMessage::Event { event });
                }
            }
            (None, Some(manager)) => manager.sweep_expired(now),
            _ => unreachable!("fake 与真实管理器互斥"),
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
