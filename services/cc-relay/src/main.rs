//! 中继二进制入口：解析环境变量并阻塞服务。
//!
//! ```text
//! CC_RELAY_TOKEN=secret CC_RELAY_PERSIST_DIR=/var/lib/cc-relay tela-cc-relay
//! ```

fn main() {
    let config = match tela_cc_relay::RelayConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("tela-cc-relay: {error}");
            std::process::exit(2);
        }
    };
    if let Some(dir) = &config.persist_dir {
        println!("tela-cc-relay: persistence enabled at {}", dir.display());
    }
    if let Err(error) = tela_cc_relay::serve(config) {
        eprintln!("tela-cc-relay: {error}");
        std::process::exit(1);
    }
}
