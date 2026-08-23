//! 控制端与目标端 DLL 共享的稳定协议。

use std::sync::atomic::{AtomicU32, AtomicU64};

pub const PROTOCOL_VERSION: u32 = 1;
pub const HEARTBEAT_TIMEOUT_MS: u64 = 2_000;
pub const NORMAL_RATE_MILLI: u64 = 1_000;
pub const SHARED_SIZE: usize = std::mem::size_of::<SharedState>();

/// 通过命名文件映射共享的状态。只放固定宽度原子值，避免跨进程布局依赖 Rust 容器。
#[repr(C)]
pub struct SharedState {
    pub version: AtomicU32,
    pub initialized: AtomicU32,
    pub rate_milli: AtomicU64,
    pub heartbeat_ms: AtomicU64,
}

impl SharedState {
    pub const fn new() -> Self {
        Self {
            version: AtomicU32::new(PROTOCOL_VERSION),
            initialized: AtomicU32::new(0),
            rate_milli: AtomicU64::new(NORMAL_RATE_MILLI),
            heartbeat_ms: AtomicU64::new(0),
        }
    }
}

impl Default for SharedState {
    fn default() -> Self {
        Self::new()
    }
}
