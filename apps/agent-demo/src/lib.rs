//! A compact tool-using agent application built with Tela.
//!
//! The application owns a bounded model/tool loop and exposes OpenAI Chat Completions compatible
//! request and response values. The selected demo model is deterministic and local, so the Web
//! product needs no API key or server while still exercising the same tool-call protocol.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod agent;
mod application;
mod presentation;
mod protocol;

pub use agent::{Agent, AgentError, MockChatModel, RunReport, RunStatus, Task, TraceEvent};
pub use application::{
    AgentAction, AgentDemoApp, AgentDemoController, DisplayMessage, DisplayRole, agent_demo_config,
    new_agent_demo,
};
pub use protocol::{
    ChatCompletionChoice, ChatCompletionModel, ChatCompletionRequest, ChatCompletionResponse,
    ChatCompletionTool, ChatMessage, FinishReason, FunctionDefinition, FunctionToolCall,
    MessageRole, ModelError, ToolCall, ToolChoice, Usage,
};
