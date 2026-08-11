//! tela-log：tela 内部日志库（零依赖）。
//!
//! 设计：
//! - **分类**（category）：启动链路 `boot` / 渲染帧 `frame` / GPU 探针 `probe` /
//!   渲染器 `render` / 其他 `wgpu` 等——telemetry 按分类过滤，不再依赖文本前缀；
//! - **输出适配器可注入**（`LogSink`）：web 注入 console sink（wasm.rs），
//!   native 注入 stderr sink，测试注入收集 sink——日志代码本身零平台依赖；
//! - **级别过滤**：`set_min_level`（默认 info；诊断类用 debug 级别避免刷屏）。
//!
//! 用法：
//! ```
//! tela_log::set_min_level(tela_log::Level::Debug);
//! tela_log::boot!("adapter 请求成功");
//! tela_log::frame!("surface=Success submit=ok");
//! tela_log::probe!("纹理创建完成");
//! tela_log::error!("wgpu", "未捕获 GPU 错误");
//! ```

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

/// 日志级别（权重：debug < info < warn < error）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

/// 一条日志记录。
pub struct Record {
    pub level: Level,
    /// 分类（编译期固定，见 crate 文档）。
    pub category: &'static str,
    pub message: String,
}

/// 输出适配器（可注入）：web console / stderr / 测试收集。
pub trait LogSink: Send + Sync {
    fn write(&self, record: &Record);
}

struct Logger {
    sink: Mutex<Option<Box<dyn LogSink>>>,
    min_level: AtomicU8,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

impl Logger {
    fn global() -> &'static Logger {
        LOGGER.get_or_init(|| Logger {
            sink: Mutex::new(None),
            min_level: AtomicU8::new(Level::Info as u8),
        })
    }

    /// 注入输出适配器（None = 关闭日志输出，日志调用零开销判断）。
    pub fn set_sink(sink: Option<Box<dyn LogSink>>) {
        *Self::global().sink.lock().unwrap() = sink;
    }

    /// 级别过滤（低于该级别丢弃）。
    pub fn set_min_level(level: Level) {
        Self::global()
            .min_level
            .store(level as u8, Ordering::Relaxed);
    }

    /// 记录一条日志（内部入口，宏调用）。
    pub fn log(level: Level, category: &'static str, message: String) {
        let logger = Self::global();
        if (level as u8) < logger.min_level.load(Ordering::Relaxed) {
            return;
        }
        let sink = logger.sink.lock().unwrap();
        if let Some(sink) = sink.as_ref() {
            sink.write(&Record {
                level,
                category,
                message,
            });
        }
    }
}

/// 模块级日志入口（宏展开目标）：`tela_log::log(Level, category, message)`。
pub fn log(level: Level, category: &'static str, message: String) {
    Logger::log(level, category, message);
}

/// 注入输出适配器（模块级入口，见 `Logger::set_sink`）。
pub fn set_sink(sink: Option<Box<dyn LogSink>>) {
    Logger::set_sink(sink);
}

/// 级别过滤（模块级入口，见 `Logger::set_min_level`）。
pub fn set_min_level(level: Level) {
    Logger::set_min_level(level);
}

/// 通用级别宏（带分类参数）：`tela_log::info!("boot", "...")`。
#[macro_export]
macro_rules! debug {
    ($cat:expr, $($arg:tt)*) => {
        $crate::log($crate::Level::Debug, $cat, format!($($arg)*))
    };
}
#[macro_export]
macro_rules! info {
    ($cat:expr, $($arg:tt)*) => {
        $crate::log($crate::Level::Info, $cat, format!($($arg)*))
    };
}
#[macro_export]
macro_rules! warn {
    ($cat:expr, $($arg:tt)*) => {
        $crate::log($crate::Level::Warn, $cat, format!($($arg)*))
    };
}
#[macro_export]
macro_rules! error {
    ($cat:expr, $($arg:tt)*) => {
        $crate::log($crate::Level::Error, $cat, format!($($arg)*))
    };
}

/// 分类宏：启动链路（GPU 初始化逐步日志）。
#[macro_export]
macro_rules! boot {
    ($($arg:tt)*) => {
        $crate::log($crate::Level::Info, "boot", format!($($arg)*))
    };
}

/// 分类宏：渲染帧状态（surface/submit/present）。
#[macro_export]
macro_rules! frame {
    ($($arg:tt)*) => {
        $crate::log($crate::Level::Info, "frame", format!($($arg)*))
    };
}

/// 分类宏：GPU 探针全步骤。
#[macro_export]
macro_rules! probe {
    ($($arg:tt)*) => {
        $crate::log($crate::Level::Info, "probe", format!($($arg)*))
    };
}

/// 分类宏：渲染器（render_frame/draw_batch 诊断，debug 级别避免刷屏）。
#[macro_export]
macro_rules! render {
    ($($arg:tt)*) => {
        $crate::log($crate::Level::Debug, "render", format!($($arg)*))
    };
}
