//! OpenAI Chat Completions compatible values used at the model boundary.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A role accepted by the Chat Completions message array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// Legacy/system-level application instructions accepted by Chat Completions clients.
    System,
    /// High-priority application instructions.
    Developer,
    /// End-user input.
    User,
    /// Model output, optionally containing tool calls.
    Assistant,
    /// Application-provided result for one prior tool call.
    Tool,
}

/// One message in an OpenAI-compatible Chat Completions request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Author role.
    pub role: MessageRole,
    /// Text content. Tool-calling assistant messages conventionally carry `null` here.
    pub content: Option<String>,
    /// Function calls requested by an assistant message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Identifier of the tool call answered by a tool message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    /// Creates a developer instruction message.
    pub fn developer(content: impl Into<String>) -> Self {
        Self::text(MessageRole::Developer, content)
    }

    /// Creates an end-user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(MessageRole::User, content)
    }

    /// Creates an assistant text message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::text(MessageRole::Assistant, content)
    }

    /// Creates an assistant message which requests one or more function calls.
    pub fn assistant_tool_calls(tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: None,
            tool_calls,
            tool_call_id: None,
        }
    }

    /// Creates an application result linked to a prior tool call.
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    fn text(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

/// A model-requested function call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Stable identifier used by the later tool result message.
    pub id: String,
    /// OpenAI tool discriminator. This demo supports `function`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Function name and JSON-encoded arguments.
    pub function: FunctionToolCall,
}

/// Function name and arguments carried by a tool call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionToolCall {
    /// Registered function name.
    pub name: String,
    /// JSON object encoded as a string, matching the Chat Completions wire shape.
    pub arguments: String,
}

/// One function tool made available to the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionTool {
    /// OpenAI tool discriminator. This demo emits `function`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Function metadata and JSON Schema.
    pub function: FunctionDefinition,
}

/// Metadata and strict JSON Schema for a callable function.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// Function name.
    pub name: String,
    /// Model-facing behavior contract.
    pub description: String,
    /// JSON Schema describing the arguments object.
    pub parameters: Value,
    /// Whether arguments must adhere exactly to the schema.
    pub strict: bool,
}

/// Model tool-selection mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoice {
    /// The model may answer directly or call tools.
    #[default]
    Auto,
    /// The model must answer without tools.
    None,
    /// The model must call at least one tool.
    Required,
}

fn default_parallel_tool_calls() -> bool {
    true
}

/// Non-streaming `POST /v1/chat/completions` request supported by the demo model boundary.
/// Optional OpenAI fields receive the API-compatible defaults when deserialized.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    /// Model identifier.
    pub model: String,
    /// Complete conversation context for this model turn.
    pub messages: Vec<ChatMessage>,
    /// Available function tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ChatCompletionTool>,
    /// Tool-selection policy.
    #[serde(default)]
    pub tool_choice: ToolChoice,
    /// Whether a model may emit multiple independent calls in one response.
    #[serde(default = "default_parallel_tool_calls")]
    pub parallel_tool_calls: bool,
    /// The demo currently implements the ordinary non-streaming response shape.
    #[serde(default)]
    pub stream: bool,
}

/// OpenAI-compatible Chat Completions response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    /// Completion identifier.
    pub id: String,
    /// Wire object discriminator, always `chat.completion`.
    pub object: String,
    /// Unix timestamp. The deterministic mock uses zero.
    pub created: u64,
    /// Model identifier which generated the response.
    pub model: String,
    /// Candidate outputs.
    pub choices: Vec<ChatCompletionChoice>,
    /// Approximate token accounting supplied by the mock.
    pub usage: Usage,
}

/// One candidate model output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatCompletionChoice {
    /// Candidate index.
    pub index: u32,
    /// Assistant message.
    pub message: ChatMessage,
    /// Why generation stopped.
    pub finish_reason: FinishReason,
}

/// Chat Completions stop reason used by this demo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// The model produced a final answer.
    Stop,
    /// The model requested application-side tool execution.
    ToolCalls,
    /// The configured output bound was reached.
    Length,
    /// A safety policy stopped output.
    ContentFilter,
}

impl FinishReason {
    /// Stable OpenAI wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::ToolCalls => "tool_calls",
            Self::Length => "length",
            Self::ContentFilter => "content_filter",
        }
    }
}

/// Token usage reported by a completion.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Approximate request tokens.
    pub prompt_tokens: u32,
    /// Approximate generated tokens.
    pub completion_tokens: u32,
    /// Sum of prompt and completion tokens.
    pub total_tokens: u32,
}

/// Failure returned by a model provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelError(String);

impl ModelError {
    /// Creates a provider error.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ModelError {}

/// Synchronous model boundary. A network provider can serialize the same request and response
/// values; the demo injects a deterministic local implementation.
pub trait ChatCompletionModel {
    /// Completes one non-streaming Chat Completions request.
    fn complete(
        &mut self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ModelError>;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn request_serializes_with_openai_function_call_shape() {
        let request = ChatCompletionRequest {
            model: "mock-openai-agent-1".to_owned(),
            messages: vec![
                ChatMessage::developer("Use tools."),
                ChatMessage::user("List tasks"),
            ],
            tools: vec![ChatCompletionTool {
                kind: "function".to_owned(),
                function: FunctionDefinition {
                    name: "list_tasks".to_owned(),
                    description: "Lists tasks".to_owned(),
                    parameters: json!({
                        "type": "object",
                        "properties": {},
                        "required": [],
                        "additionalProperties": false
                    }),
                    strict: true,
                },
            }],
            tool_choice: ToolChoice::Auto,
            parallel_tool_calls: true,
            stream: false,
        };

        let value = serde_json::to_value(request).expect("serialize request");
        assert_eq!(value["model"], "mock-openai-agent-1");
        assert_eq!(value["messages"][0]["role"], "developer");
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][0]["function"]["strict"], true);
        assert_eq!(value["tool_choice"], "auto");
        assert_eq!(value["parallel_tool_calls"], true);
        assert_eq!(value["stream"], false);
    }

    #[test]
    fn tool_result_preserves_the_call_identifier() {
        let value = serde_json::to_value(ChatMessage::tool("call_42", r#"{"ok":true}"#))
            .expect("serialize tool result");
        assert_eq!(value["role"], "tool");
        assert_eq!(value["tool_call_id"], "call_42");
        assert_eq!(value["content"], r#"{"ok":true}"#);
    }

    #[test]
    fn minimal_openai_request_uses_protocol_defaults() {
        let request: ChatCompletionRequest = serde_json::from_value(json!({
            "model": "mock-openai-agent-1",
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .expect("deserialize minimal request");

        assert!(request.tools.is_empty());
        assert_eq!(request.tool_choice, ToolChoice::Auto);
        assert!(request.parallel_tool_calls);
        assert!(!request.stream);
    }
}
