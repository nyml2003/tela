//! Static browser product assembly for Tela Agent Demo.
//!
//! The application, session runtime, WebView target, renderer, resources, and mock provider are
//! linked into one WebAssembly module. JavaScript only drives this concrete session and never
//! imports or instantiates a second guest module.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use tela_agent_demo::{AgentDemoApp, new_agent_demo};
use tela_app_abi::{
    AppDispatchOutcome, AppFrameToken, ApplicationSession, FrameTransportSender, decode_event,
};
use tela_contract::UiResourceSet;
use tela_icon_resources::MaterialIconFontProvider;
use tela_text_resources::ControlledTextMeasurer;
use wasm_bindgen::prelude::*;

static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider);

/// Browser-visible result of a session lifecycle operation.
#[wasm_bindgen]
pub struct AgentDispatchOutcome {
    outcome: AppDispatchOutcome,
}

#[wasm_bindgen]
impl AgentDispatchOutcome {
    /// Whether the application accepted the delivered event.
    #[wasm_bindgen(getter)]
    pub fn handled(&self) -> bool {
        self.outcome.handled
    }

    /// Whether the browser must request and present a fresh publication.
    #[wasm_bindgen(getter)]
    pub fn publish_requested(&self) -> bool {
        self.outcome.publish_requested
    }
}

/// Concrete statically linked Tela application session owned by the browser page.
#[wasm_bindgen]
pub struct AgentWebSession {
    app: AgentDemoApp,
    pending: Option<AppFrameToken>,
    transport: FrameTransportSender,
    pending_transport_sequence: Option<u64>,
    initialized: bool,
    closed: bool,
}

impl Default for AgentWebSession {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl AgentWebSession {
    /// Creates a fresh local agent, tool store, and Tela application runtime.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            app: new_agent_demo(&RESOURCES),
            pending: None,
            transport: FrameTransportSender::default(),
            pending_transport_sequence: None,
            initialized: false,
            closed: false,
        }
    }

    /// Initializes the application lifecycle and requests its first frame.
    pub fn initialize(&mut self) -> Result<AgentDispatchOutcome, JsValue> {
        self.ensure_open()?;
        if self.initialized {
            return Err(js_error("agent session is already initialized"));
        }
        let outcome = ApplicationSession::initialize(&mut self.app)
            .map(outcome)
            .map_err(|error| js_error(error.to_string()))?;
        self.initialized = true;
        Ok(outcome)
    }

    /// Decodes and dispatches one event packet created by the linked WebView target.
    pub fn dispatch(&mut self, packet: &[u8]) -> Result<AgentDispatchOutcome, JsValue> {
        self.ensure_ready()?;
        let event = decode_event(packet)
            .map_err(|error| js_error(format!("invalid host event packet: {error}")))?;
        ApplicationSession::dispatch(&mut self.app, event)
            .map(outcome)
            .map_err(|error| js_error(error.to_string()))
    }

    /// Builds the newest retained-frame transport packet for JavaScript.
    pub fn publish(&mut self) -> Result<tela_target_webview::WebAppPublication, JsValue> {
        self.ensure_ready()?;
        if let Some(token) = self.pending.take() {
            ApplicationSession::rejected(&mut self.app, token);
            if let Some(sequence) = self.pending_transport_sequence.take() {
                self.transport.reject(sequence);
            }
        }
        let publication = ApplicationSession::publish(&mut self.app)
            .map_err(|error| js_error(error.to_string()))?;
        self.pending = Some(publication.token);
        let packet = self.transport.publish(
            publication.token,
            &publication.frame,
            &publication.damage,
            &publication.spine,
            publication.retained_tree.clone(),
        );
        self.pending_transport_sequence = Some(packet.sequence());
        tela_target_webview::web_app_publication_from_transport(packet, publication.status)
    }

    /// Commits a frame only after the WebGPU surface reports successful presentation.
    pub fn presented(&mut self, raw_token: u64) -> Result<AgentDispatchOutcome, JsValue> {
        self.ensure_ready()?;
        let token = AppFrameToken::new(raw_token)
            .ok_or_else(|| js_error("presented token must be non-zero"))?;
        if self.pending != Some(token) {
            return Err(js_error("presented token is not the pending publication"));
        }
        let result = ApplicationSession::presented(&mut self.app, token)
            .map(outcome)
            .map_err(|error| js_error(error.to_string()))?;
        let sequence = self
            .pending_transport_sequence
            .take()
            .ok_or_else(|| js_error("presented publication has no transport sequence"))?;
        self.transport.acknowledge(sequence);
        self.pending = None;
        let effects = ApplicationSession::take_presented_effects(&mut self.app);
        if !effects.is_empty() {
            return Err(js_error(format!(
                "browser host does not support committed native effects: {effects:?}"
            )));
        }
        Ok(result)
    }

    /// Rejects a pending publication which could not be drawn.
    pub fn rejected(&mut self, raw_token: u64) -> Result<(), JsValue> {
        self.ensure_ready()?;
        let token = AppFrameToken::new(raw_token)
            .ok_or_else(|| js_error("rejected token must be non-zero"))?;
        if self.pending != Some(token) {
            return Err(js_error("rejected token is not the pending publication"));
        }
        ApplicationSession::rejected(&mut self.app, token);
        self.pending = None;
        if let Some(sequence) = self.pending_transport_sequence.take() {
            self.transport.reject(sequence);
        }
        Ok(())
    }

    /// Releases application-owned and pending frame state.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        if let Some(token) = self.pending.take() {
            ApplicationSession::rejected(&mut self.app, token);
        }
        if let Some(sequence) = self.pending_transport_sequence.take() {
            self.transport.reject(sequence);
        }
        ApplicationSession::close(&mut self.app);
        self.closed = true;
    }
}

impl AgentWebSession {
    fn ensure_open(&self) -> Result<(), JsValue> {
        if self.closed {
            Err(js_error("agent session is closed"))
        } else {
            Ok(())
        }
    }

    fn ensure_ready(&self) -> Result<(), JsValue> {
        self.ensure_open()?;
        if self.initialized {
            Ok(())
        } else {
            Err(js_error("agent session is not initialized"))
        }
    }
}

fn outcome(outcome: AppDispatchOutcome) -> AgentDispatchOutcome {
    AgentDispatchOutcome { outcome }
}

fn js_error(message: impl AsRef<str>) -> JsValue {
    JsValue::from_str(message.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_session_initializes_and_publishes_without_a_guest_module() {
        let mut session = AgentWebSession::new();
        assert!(
            session
                .initialize()
                .expect("initialize")
                .publish_requested()
        );
        let publication = session.publish().expect("publication");
        let status = publication.status();
        let token = status.frame_token().expect("initial token");
        assert!(publication.transport_snapshot());
        assert_eq!(publication.transport_base_sequence(), None);

        session.presented(token).expect("initial presented");
        let viewport = tela_target_webview::event_viewport(800.0, 600.0).expect("viewport");
        session.dispatch(&viewport).expect("dispatch viewport");
        let patch = session.publish().expect("patch publication");
        assert!(!patch.transport_snapshot());
        assert_eq!(patch.transport_base_sequence(), Some(1));
        assert!(!patch.transport_spine().is_empty());
    }
}
