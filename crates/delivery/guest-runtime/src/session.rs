//! 把 [`GuestRuntime`] 适配成 [`ApplicationSession`]。
//!
//! WASM guest 与进程内应用（`tela-app-runtime::Application` 等）由此共用同一套壳协议：
//! 宿主壳只面对一个会话对象，不再感知执行器差异。本模块不含任何平台窗口类型，
//! macOS/Android 等宿主可直接复用。

use tela_app_abi::{
    AppDispatchOutcome, AppEffect, AppEvent, AppFrameToken, AppPublication, ApplicationSession,
    SessionError,
};
use tela_bridge::{BridgeDispatcher, BridgeEvent};

use crate::runtime::GuestRuntime;

/// WASM guest 的 [`ApplicationSession`] 适配器。
///
/// [`GuestRuntime::new`] 已在后台线程完成 initialize 与首次发布；首帧经
/// [`GuestSession::new`] 缓存为 `initial`，壳的第一次 `publish` 不重入 WASM。
pub struct GuestSession {
    runtime: GuestRuntime,
    initial: Option<AppPublication>,
    /// Effects returned by acknowledged guest publications since the last session-level drain.
    /// Keeping them here preserves the same lossless one-shot drain contract as an in-process
    /// application even when a host delays its drain across a follow-up publication.
    presented_effects: Vec<AppEffect>,
    /// 宿主桥：每帧派发后排空 guest 请求并投递响应（等价 desktop-runtime 的
    /// `process_bridge_requests`；本 crate 不能依赖 desktop-runtime，故在此复刻）。
    bridge: Option<BridgeDispatcher>,
}

impl GuestSession {
    /// 用已初始化的 runtime 组装会话；`bridge` 为 `None` 时跳过桥泵。
    pub fn new(runtime: GuestRuntime, bridge: Option<BridgeDispatcher>) -> Result<Self, String> {
        let initial = runtime
            .pending_publication()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            runtime,
            initial: Some(initial),
            presented_effects: Vec::new(),
            bridge,
        })
    }

    /// 透出编译/初始化/燃料指标（诊断入口）。
    pub fn metrics(&self) -> crate::runtime::GuestRuntimeMetrics {
        self.runtime.metrics()
    }

    /// 只读访问内部 runtime（宿主特殊诊断用；常规交互请走 [`ApplicationSession`]）。
    pub fn runtime(&self) -> &GuestRuntime {
        &self.runtime
    }

    fn pump_bridge(&mut self) -> Result<(), String> {
        if !self.runtime.bridge_available() {
            return Ok(());
        }
        let requests = self
            .runtime
            .bridge_drain_requests()
            .map_err(|error| error.to_string())?;
        let events: Vec<BridgeEvent> = {
            let Some(dispatcher) = self.bridge.as_mut() else {
                return Ok(());
            };
            requests
                .into_iter()
                .filter_map(|request| dispatcher.handle(request))
                .collect()
        };
        for event in events {
            self.deliver_bridge_event(&event)?;
        }
        Ok(())
    }

    fn deliver_bridge_event(&mut self, event: &BridgeEvent) -> Result<(), String> {
        let packet = tela_bridge::encode_event(event).map_err(|error| error.to_string())?;
        self.runtime
            .bridge_deliver(&packet)
            .map_err(|error| error.to_string())
    }
}

impl ApplicationSession for GuestSession {
    fn initialize(&mut self) -> Result<AppDispatchOutcome, SessionError> {
        // GuestRuntime::new 已完成 initialize 与首次发布；向壳声明一次发布请求即可。
        Ok(AppDispatchOutcome {
            handled: true,
            publish_requested: true,
        })
    }

    fn dispatch(&mut self, event: AppEvent) -> Result<AppDispatchOutcome, SessionError> {
        let outcome = self
            .runtime
            .dispatch(&event)
            .map_err(|error| SessionError::new(error.to_string()))?;
        self.pump_bridge().map_err(SessionError::new)?;
        Ok(outcome)
    }

    fn publish(&mut self) -> Result<AppPublication, SessionError> {
        match self.initial.take() {
            Some(first) => Ok(first),
            None => self
                .runtime
                .publish_latest()
                .map_err(|error| SessionError::new(error.to_string())),
        }
    }

    fn presented(&mut self, token: AppFrameToken) -> Result<AppDispatchOutcome, SessionError> {
        let acknowledged = self
            .runtime
            .presented(token)
            .map_err(|error| SessionError::new(error.to_string()))?;
        self.presented_effects.extend(acknowledged.effects);
        Ok(acknowledged.outcome)
    }

    fn take_presented_effects(&mut self) -> Vec<AppEffect> {
        std::mem::take(&mut self.presented_effects)
    }

    fn rejected(&mut self, token: AppFrameToken) {
        if let Err(error) = self.runtime.rejected(token) {
            eprintln!("tela-guest-session: rejected failed: {error}");
        }
    }

    fn close(&mut self) {
        // guest ABI 没有 close 导出；实例随宿主壳的会话槽一起销毁。
    }
}
