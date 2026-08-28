//! Bounded agent loop, deterministic model, and local function tools.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::protocol::{
    ChatCompletionChoice, ChatCompletionModel, ChatCompletionRequest, ChatCompletionResponse,
    ChatCompletionTool, ChatMessage, FinishReason, FunctionDefinition, FunctionToolCall,
    MessageRole, ModelError, ToolCall, ToolChoice, Usage,
};

const DEFAULT_MODEL: &str = "mock-openai-agent-1";
const DEFAULT_MAX_ROUNDS: usize = 6;
const DEVELOPER_INSTRUCTION: &str = "You are a concise task agent. Use the supplied functions when they can establish facts or change task state. Continue after tool results until you can return a final answer.";

/// One task held by the demo's stateful local tool runtime.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    /// Stable task identifier.
    pub id: String,
    /// User-visible task title.
    pub title: String,
    /// Lightweight priority label.
    pub priority: String,
    /// Current task state.
    pub status: String,
}

/// High-level outcome of one complete agent run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    /// A final assistant response was produced.
    Completed,
}

/// Auditable events emitted by the model/tool loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceEvent {
    /// A Chat Completions request was submitted to the provider boundary.
    ModelRequest {
        /// One-based round number within this run.
        round: usize,
        /// Messages sent with the request.
        messages: usize,
        /// Available function tools.
        tools: usize,
    },
    /// The provider returned one assistant choice.
    ModelResponse {
        /// One-based round number within this run.
        round: usize,
        /// OpenAI-compatible finish reason.
        finish_reason: FinishReason,
        /// Number of requested function calls.
        tool_calls: usize,
    },
    /// The model requested an application-side function.
    ToolCall {
        /// Call identifier used to link the result.
        id: String,
        /// Function name.
        name: String,
        /// JSON-encoded arguments.
        arguments: String,
    },
    /// The application returned a function result to the model context.
    ToolResult {
        /// Call identifier copied from the request.
        id: String,
        /// Function name.
        name: String,
        /// Compact JSON result.
        content: String,
        /// Whether function execution failed.
        is_error: bool,
    },
    /// A final answer satisfied the stop condition.
    Completed {
        /// Number of model rounds consumed.
        rounds: usize,
        /// Number of function calls executed.
        tool_calls: usize,
    },
}

/// Complete result of one user goal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    /// Monotonic run identifier.
    pub id: u64,
    /// Original user goal.
    pub goal: String,
    /// Final assistant text.
    pub answer: String,
    /// Terminal status.
    pub status: RunStatus,
    /// Number of model rounds.
    pub rounds: usize,
    /// Number of tool calls.
    pub tool_calls: usize,
    /// Ordered execution trace.
    pub trace: Vec<TraceEvent>,
}

/// Agent orchestration failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentError {
    /// Provider call failed.
    Model(String),
    /// Provider returned a response inconsistent with the Chat Completions contract.
    InvalidResponse(String),
    /// The model did not reach a final answer within the configured bound.
    IterationLimit {
        /// Enforced maximum model rounds.
        max_rounds: usize,
    },
    /// The submitted goal was empty.
    EmptyGoal,
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model(message) => write!(formatter, "model request failed: {message}"),
            Self::InvalidResponse(message) => {
                write!(formatter, "invalid model response: {message}")
            }
            Self::IterationLimit { max_rounds } => {
                write!(formatter, "agent exceeded its {max_rounds}-round limit")
            }
            Self::EmptyGoal => formatter.write_str("agent goal cannot be empty"),
        }
    }
}

impl std::error::Error for AgentError {}

/// A model-driven agent with persistent conversation and tool state.
pub struct Agent<M> {
    model: M,
    model_id: String,
    history: Vec<ChatMessage>,
    tools: ToolRuntime,
    max_rounds: usize,
    next_run_id: u64,
}

impl<M: ChatCompletionModel> Agent<M> {
    /// Creates an agent with the default bounded orchestration policy.
    pub fn new(model: M, model_id: impl Into<String>) -> Self {
        Self {
            model,
            model_id: model_id.into(),
            history: vec![ChatMessage::developer(DEVELOPER_INSTRUCTION)],
            tools: ToolRuntime::default(),
            max_rounds: DEFAULT_MAX_ROUNDS,
            next_run_id: 0,
        }
    }

    /// Overrides the round bound. Values below one are clamped to one.
    pub fn with_max_rounds(mut self, max_rounds: usize) -> Self {
        self.max_rounds = max_rounds.max(1);
        self
    }

    /// Runs the observe-decide-act loop until a final answer or the configured bound.
    pub fn run(&mut self, goal: impl Into<String>) -> Result<RunReport, AgentError> {
        let goal = goal.into().trim().to_owned();
        if goal.is_empty() {
            return Err(AgentError::EmptyGoal);
        }
        self.next_run_id = self.next_run_id.saturating_add(1);
        let run_id = self.next_run_id;
        self.history.push(ChatMessage::user(goal.clone()));
        let mut trace = Vec::new();
        let mut executed_calls = 0;

        for round in 1..=self.max_rounds {
            let request = ChatCompletionRequest {
                model: self.model_id.clone(),
                messages: self.history.clone(),
                tools: self.tools.definitions(),
                tool_choice: ToolChoice::Auto,
                parallel_tool_calls: true,
                stream: false,
            };
            trace.push(TraceEvent::ModelRequest {
                round,
                messages: request.messages.len(),
                tools: request.tools.len(),
            });
            let response = self
                .model
                .complete(&request)
                .map_err(|error| AgentError::Model(error.to_string()))?;
            let choice = response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| AgentError::InvalidResponse("choices is empty".to_owned()))?;
            validate_choice(&choice)?;
            let tool_calls = choice.message.tool_calls.clone();
            trace.push(TraceEvent::ModelResponse {
                round,
                finish_reason: choice.finish_reason,
                tool_calls: tool_calls.len(),
            });
            self.history.push(choice.message.clone());

            if tool_calls.is_empty() {
                let answer = choice.message.content.ok_or_else(|| {
                    AgentError::InvalidResponse("final assistant content is null".to_owned())
                })?;
                trace.push(TraceEvent::Completed {
                    rounds: round,
                    tool_calls: executed_calls,
                });
                return Ok(RunReport {
                    id: run_id,
                    goal,
                    answer,
                    status: RunStatus::Completed,
                    rounds: round,
                    tool_calls: executed_calls,
                    trace,
                });
            }

            for call in tool_calls {
                trace.push(TraceEvent::ToolCall {
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    arguments: call.function.arguments.clone(),
                });
                let result = self.tools.execute(&call);
                executed_calls += 1;
                trace.push(TraceEvent::ToolResult {
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    content: result.content.clone(),
                    is_error: result.is_error,
                });
                self.history
                    .push(ChatMessage::tool(call.id, result.content));
            }
        }

        Err(AgentError::IterationLimit {
            max_rounds: self.max_rounds,
        })
    }

    /// Current task state owned by the function-tool runtime.
    pub fn tasks(&self) -> &[Task] {
        &self.tools.tasks
    }

    /// Full OpenAI-compatible conversation context, including tool messages.
    pub fn history(&self) -> &[ChatMessage] {
        &self.history
    }
}

fn validate_choice(choice: &ChatCompletionChoice) -> Result<(), AgentError> {
    if choice.message.role != MessageRole::Assistant {
        return Err(AgentError::InvalidResponse(
            "choice message role must be assistant".to_owned(),
        ));
    }
    for call in &choice.message.tool_calls {
        if call.kind != "function" {
            return Err(AgentError::InvalidResponse(format!(
                "unsupported tool call type `{}`",
                call.kind
            )));
        }
        if call.id.trim().is_empty() {
            return Err(AgentError::InvalidResponse(
                "tool call id cannot be empty".to_owned(),
            ));
        }
        if call.function.name.trim().is_empty() {
            return Err(AgentError::InvalidResponse(
                "tool call function name cannot be empty".to_owned(),
            ));
        }
    }
    let has_calls = !choice.message.tool_calls.is_empty();
    match (has_calls, choice.finish_reason) {
        (true, FinishReason::ToolCalls) | (false, FinishReason::Stop) => Ok(()),
        (true, reason) => Err(AgentError::InvalidResponse(format!(
            "assistant returned tool calls with finish_reason={}",
            reason.as_str()
        ))),
        (false, reason) => Err(AgentError::InvalidResponse(format!(
            "assistant returned no tool calls with finish_reason={}",
            reason.as_str()
        ))),
    }
}

#[derive(Default)]
struct ToolRuntime {
    tasks: Vec<Task>,
}

struct ToolExecution {
    content: String,
    is_error: bool,
}

impl ToolRuntime {
    fn definitions(&self) -> Vec<ChatCompletionTool> {
        vec![
            function_tool(
                "create_task",
                "Create one persistent task in the current agent session.",
                json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Concrete task title" },
                        "priority": { "type": "string", "enum": ["high", "normal", "low"] }
                    },
                    "required": ["title", "priority"],
                    "additionalProperties": false
                }),
            ),
            function_tool(
                "list_tasks",
                "List every task in the current agent session.",
                json!({
                    "type": "object",
                    "properties": {},
                    "required": [],
                    "additionalProperties": false
                }),
            ),
            function_tool(
                "inspect_workspace",
                "Inspect the built-in Tela Agent Demo runtime and delivery state.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "State to inspect" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            ),
        ]
    }

    fn execute(&mut self, call: &ToolCall) -> ToolExecution {
        let arguments: Value = match serde_json::from_str(&call.function.arguments) {
            Ok(arguments) => arguments,
            Err(error) => return tool_error(format!("invalid JSON arguments: {error}")),
        };
        match call.function.name.as_str() {
            "create_task" => {
                let Some(title) = arguments.get("title").and_then(Value::as_str) else {
                    return tool_error("create_task requires string field `title`");
                };
                let title = title.trim();
                if title.is_empty() {
                    return tool_error("create_task title cannot be empty");
                }
                let priority = arguments
                    .get("priority")
                    .and_then(Value::as_str)
                    .unwrap_or("normal");
                if !matches!(priority, "high" | "normal" | "low") {
                    return tool_error("create_task priority must be high, normal, or low");
                }
                let task = Task {
                    id: format!("task-{:03}", self.tasks.len() + 1),
                    title: title.to_owned(),
                    priority: priority.to_owned(),
                    status: "ready".to_owned(),
                };
                self.tasks.push(task.clone());
                tool_ok(json!({ "ok": true, "task": task }))
            }
            "list_tasks" => tool_ok(json!({
                "ok": true,
                "count": self.tasks.len(),
                "tasks": self.tasks
            })),
            "inspect_workspace" => tool_ok(json!({
                "ok": true,
                "project": "tela",
                "application": "Tela Agent Demo",
                "model": DEFAULT_MODEL,
                "protocol": "POST /v1/chat/completions",
                "delivery": "static-link",
                "runtime": "single wasm32-unknown-unknown module",
                "dynamic_guest_import": false,
                "tools": 3,
                "task_count": self.tasks.len()
            })),
            name => tool_error(format!("unknown function tool `{name}`")),
        }
    }
}

fn function_tool(name: &str, description: &str, parameters: Value) -> ChatCompletionTool {
    ChatCompletionTool {
        kind: "function".to_owned(),
        function: FunctionDefinition {
            name: name.to_owned(),
            description: description.to_owned(),
            parameters,
            strict: true,
        },
    }
}

fn tool_ok(value: Value) -> ToolExecution {
    ToolExecution {
        content: value.to_string(),
        is_error: false,
    }
}

fn tool_error(message: impl Into<String>) -> ToolExecution {
    ToolExecution {
        content: json!({ "ok": false, "error": message.into() }).to_string(),
        is_error: true,
    }
}

/// Deterministic local provider which emits OpenAI-compatible assistant and function-call choices.
#[derive(Default)]
pub struct MockChatModel {
    completion_sequence: u64,
    call_sequence: u64,
}

impl MockChatModel {
    /// Creates a fresh deterministic model.
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles one JSON request body and returns an OpenAI-compatible JSON response body.
    ///
    /// A real HTTP adapter can put this method behind `POST /v1/chat/completions`; keeping the
    /// serialization here makes the mock useful in tests and leaves transport policy outside the
    /// agent loop.
    pub fn complete_json(&mut self, body: &str) -> Result<String, ModelError> {
        let request = serde_json::from_str::<ChatCompletionRequest>(body).map_err(|error| {
            ModelError::new(format!("invalid chat completion request: {error}"))
        })?;
        let response = self.complete(&request)?;
        serde_json::to_string(&response)
            .map_err(|error| ModelError::new(format!("encode chat completion response: {error}")))
    }

    fn tool_call(&mut self, name: &str, arguments: Value) -> ToolCall {
        self.call_sequence = self.call_sequence.saturating_add(1);
        ToolCall {
            id: format!("call_mock_{:04}", self.call_sequence),
            kind: "function".to_owned(),
            function: FunctionToolCall {
                name: name.to_owned(),
                arguments: arguments.to_string(),
            },
        }
    }

    fn response(
        &mut self,
        request: &ChatCompletionRequest,
        message: ChatMessage,
        finish_reason: FinishReason,
    ) -> ChatCompletionResponse {
        self.completion_sequence = self.completion_sequence.saturating_add(1);
        let prompt_tokens = approximate_tokens(
            &request
                .messages
                .iter()
                .filter_map(|message| message.content.as_deref())
                .collect::<Vec<_>>()
                .join(" "),
        );
        let completion_text = message.content.as_deref().unwrap_or_else(|| {
            message
                .tool_calls
                .first()
                .map_or("", |call| call.function.arguments.as_str())
        });
        let completion_tokens = approximate_tokens(completion_text);
        ChatCompletionResponse {
            id: format!("chatcmpl-mock-{:04}", self.completion_sequence),
            object: "chat.completion".to_owned(),
            created: 0,
            model: request.model.clone(),
            choices: vec![ChatCompletionChoice {
                index: 0,
                message,
                finish_reason,
            }],
            usage: Usage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens.saturating_add(completion_tokens),
            },
        }
    }
}

impl ChatCompletionModel for MockChatModel {
    fn complete(
        &mut self,
        request: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, ModelError> {
        if request.stream {
            return Err(ModelError::new(
                "mock provider supports non-streaming requests only",
            ));
        }
        if request.messages.is_empty() {
            return Err(ModelError::new("messages cannot be empty"));
        }
        if request
            .messages
            .last()
            .is_some_and(|message| message.role == MessageRole::Tool)
        {
            let answer = summarize_recent_tool_results(&request.messages);
            return Ok(self.response(request, ChatMessage::assistant(answer), FinishReason::Stop));
        }

        let goal = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::User)
            .and_then(|message| message.content.as_deref())
            .ok_or_else(|| ModelError::new("request has no user message"))?;
        let can_call = |name: &str| {
            request
                .tools
                .iter()
                .any(|tool| tool.kind == "function" && tool.function.name == name)
        };
        let mut calls = if request.tool_choice == ToolChoice::None {
            Vec::new()
        } else if goal.contains("列出") && goal.contains("任务") && can_call("list_tasks") {
            vec![self.tool_call("list_tasks", json!({}))]
        } else if goal.contains("任务")
            && (goal.contains("创建") || goal.contains("添加") || goal.contains("安排"))
            && can_call("create_task")
        {
            extract_task_titles(goal)
                .into_iter()
                .enumerate()
                .map(|(index, title)| {
                    self.tool_call(
                        "create_task",
                        json!({
                            "title": title,
                            "priority": if index == 0 { "high" } else { "normal" }
                        }),
                    )
                })
                .collect()
        } else if (goal.contains("状态") || goal.to_ascii_lowercase().contains("tela"))
            && can_call("inspect_workspace")
        {
            vec![self.tool_call("inspect_workspace", json!({ "query": goal }))]
        } else {
            Vec::new()
        };
        if calls.is_empty() && request.tool_choice == ToolChoice::Required {
            let fallback = if can_call("inspect_workspace") {
                self.tool_call("inspect_workspace", json!({ "query": goal }))
            } else if can_call("list_tasks") {
                self.tool_call("list_tasks", json!({}))
            } else if can_call("create_task") {
                self.tool_call(
                    "create_task",
                    json!({ "title": goal, "priority": "normal" }),
                )
            } else {
                return Err(ModelError::new(
                    "tool_choice=required but no supported function tool is available",
                ));
            };
            calls.push(fallback);
        }
        if !request.parallel_tool_calls {
            calls.truncate(1);
        }

        if calls.is_empty() {
            let answer =
                format!("目标已分析：{goal}。当前不需要调用工具；mock 模型已直接完成这一轮。");
            Ok(self.response(request, ChatMessage::assistant(answer), FinishReason::Stop))
        } else {
            Ok(self.response(
                request,
                ChatMessage::assistant_tool_calls(calls),
                FinishReason::ToolCalls,
            ))
        }
    }
}

fn extract_task_titles(goal: &str) -> Vec<String> {
    let body = goal
        .split_once('：')
        .or_else(|| goal.split_once(':'))
        .map_or(goal, |(_, body)| body);
    let parts: Vec<_> = if body.contains("，再") {
        body.split("，再").collect()
    } else if body.contains(", then ") {
        body.split(", then ").collect()
    } else if body.contains("再") {
        body.split('再').collect()
    } else {
        vec![body]
    };
    let mut titles: Vec<String> = parts
        .into_iter()
        .map(|part| {
            part.trim()
                .trim_start_matches('先')
                .trim_start_matches("创建")
                .trim_start_matches("添加")
                .trim_matches(|character: char| {
                    character.is_whitespace() || matches!(character, '，' | ',' | '。' | '.')
                })
                .to_owned()
        })
        .filter(|title| !title.is_empty())
        .collect();
    if titles.is_empty() {
        titles.push("处理用户目标".to_owned());
    }
    titles.truncate(4);
    titles
}

fn summarize_recent_tool_results(messages: &[ChatMessage]) -> String {
    let Some(calls) = messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::Assistant && !message.tool_calls.is_empty())
        .map(|message| message.tool_calls.as_slice())
    else {
        return "工具结果已接收，但缺少对应的 assistant tool_calls。".to_owned();
    };
    let mut created = Vec::new();
    let mut listed = Vec::new();
    let mut inspected = false;
    let mut errors = Vec::new();

    for call in calls {
        let result = messages.iter().rev().find(|message| {
            message.role == MessageRole::Tool
                && message.tool_call_id.as_deref() == Some(call.id.as_str())
        });
        let value = result
            .and_then(|message| message.content.as_deref())
            .and_then(|content| serde_json::from_str::<Value>(content).ok())
            .unwrap_or_else(|| json!({ "ok": false, "error": "missing tool result" }));
        if value.get("ok").and_then(Value::as_bool) != Some(true) {
            errors.push(
                value
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown tool error")
                    .to_owned(),
            );
            continue;
        }
        match call.function.name.as_str() {
            "create_task" => {
                if let Some(title) = value
                    .get("task")
                    .and_then(|task| task.get("title"))
                    .and_then(Value::as_str)
                {
                    created.push(title.to_owned());
                }
            }
            "list_tasks" => {
                if let Some(tasks) = value.get("tasks").and_then(Value::as_array) {
                    listed.extend(tasks.iter().filter_map(|task| {
                        task.get("title").and_then(Value::as_str).map(str::to_owned)
                    }));
                }
            }
            "inspect_workspace" => inspected = true,
            _ => {}
        }
    }

    if !errors.is_empty() {
        return format!("工具执行未完成：{}。", errors.join("；"));
    }
    if !created.is_empty() {
        return format!(
            "已创建 {} 个任务：{}。任务保存在当前 Wasm 会话中。",
            created.len(),
            created.join("；")
        );
    }
    if !listed.is_empty() {
        return format!("当前有 {} 个任务：{}。", listed.len(), listed.join("；"));
    }
    if calls.iter().any(|call| call.function.name == "list_tasks") {
        return "当前没有任务。".to_owned();
    }
    if inspected {
        return "检查完成：Tela Agent Demo 正以单 Wasm 静态链接方式运行，模型边界为 POST /v1/chat/completions，动态 guest 导入已关闭。".to_owned();
    }
    "工具调用完成，结果已回灌给模型。".to_owned()
}

fn approximate_tokens(text: &str) -> u32 {
    u32::try_from(text.chars().count().div_ceil(4).max(1)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_executes_parallel_calls_and_returns_results_to_the_model() {
        let mut agent = Agent::new(MockChatModel::new(), DEFAULT_MODEL);
        let report = agent
            .run("创建两个任务：完成 Tela Agent Demo，再检查浏览器验收")
            .expect("agent run");

        assert_eq!(report.status, RunStatus::Completed);
        assert_eq!(report.rounds, 2);
        assert_eq!(report.tool_calls, 2);
        assert_eq!(agent.tasks().len(), 2);
        assert!(report.answer.contains("完成 Tela Agent Demo"));
        assert!(report.answer.contains("检查浏览器验收"));
        assert!(matches!(
            report.trace.as_slice(),
            [
                TraceEvent::ModelRequest { round: 1, .. },
                TraceEvent::ModelResponse {
                    finish_reason: FinishReason::ToolCalls,
                    tool_calls: 2,
                    ..
                },
                TraceEvent::ToolCall { .. },
                TraceEvent::ToolResult { .. },
                TraceEvent::ToolCall { .. },
                TraceEvent::ToolResult { .. },
                TraceEvent::ModelRequest { round: 2, .. },
                TraceEvent::ModelResponse {
                    finish_reason: FinishReason::Stop,
                    tool_calls: 0,
                    ..
                },
                TraceEvent::Completed {
                    rounds: 2,
                    tool_calls: 2
                }
            ]
        ));
    }

    #[test]
    fn persistent_tool_state_is_visible_to_a_later_goal() {
        let mut agent = Agent::new(MockChatModel::new(), DEFAULT_MODEL);
        agent
            .run("添加任务：验证 OpenAI 协议")
            .expect("create task");
        let report = agent.run("列出当前任务").expect("list tasks");

        assert_eq!(report.tool_calls, 1);
        assert!(report.answer.contains("验证 OpenAI 协议"));
        assert!(agent.history().iter().any(|message| {
            message.role == MessageRole::Tool && message.tool_call_id.is_some()
        }));
    }

    struct LoopingModel {
        call: u64,
    }

    impl ChatCompletionModel for LoopingModel {
        fn complete(
            &mut self,
            request: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, ModelError> {
            self.call += 1;
            let message = ChatMessage::assistant_tool_calls(vec![ToolCall {
                id: format!("call_loop_{}", self.call),
                kind: "function".to_owned(),
                function: FunctionToolCall {
                    name: "list_tasks".to_owned(),
                    arguments: "{}".to_owned(),
                },
            }]);
            Ok(ChatCompletionResponse {
                id: format!("loop-{}", self.call),
                object: "chat.completion".to_owned(),
                created: 0,
                model: request.model.clone(),
                choices: vec![ChatCompletionChoice {
                    index: 0,
                    message,
                    finish_reason: FinishReason::ToolCalls,
                }],
                usage: Usage::default(),
            })
        }
    }

    #[test]
    fn round_limit_is_a_real_stop_condition() {
        let mut agent = Agent::new(LoopingModel { call: 0 }, "loop").with_max_rounds(2);
        assert_eq!(
            agent.run("keep going"),
            Err(AgentError::IterationLimit { max_rounds: 2 })
        );
    }

    #[test]
    fn json_endpoint_round_trips_a_minimal_openai_request() {
        let mut model = MockChatModel::new();
        let body = serde_json::json!({
            "model": DEFAULT_MODEL,
            "messages": [{"role": "user", "content": "hello"}]
        })
        .to_string();

        let response: serde_json::Value =
            serde_json::from_str(&model.complete_json(&body).expect("json completion"))
                .expect("response JSON");
        assert_eq!(response["object"], "chat.completion");
        assert_eq!(response["choices"][0]["finish_reason"], "stop");
        assert!(response["choices"][0]["message"]["tool_calls"].is_null());
    }

    #[test]
    fn tool_choice_and_parallel_call_policy_are_enforced() {
        let tools = ToolRuntime::default().definitions();
        let mut model = MockChatModel::new();
        let required = ChatCompletionRequest {
            model: DEFAULT_MODEL.to_owned(),
            messages: vec![ChatMessage::user("hello")],
            tools: tools.clone(),
            tool_choice: ToolChoice::Required,
            parallel_tool_calls: true,
            stream: false,
        };
        let response = model.complete(&required).expect("required tool response");
        assert_eq!(response.choices[0].finish_reason, FinishReason::ToolCalls);
        assert_eq!(response.choices[0].message.tool_calls.len(), 1);

        let serial = ChatCompletionRequest {
            model: DEFAULT_MODEL.to_owned(),
            messages: vec![ChatMessage::user(
                "创建两个任务：完成 Tela Agent Demo，再检查浏览器验收",
            )],
            tools,
            tool_choice: ToolChoice::Auto,
            parallel_tool_calls: false,
            stream: false,
        };
        let response = model.complete(&serial).expect("serial tool response");
        assert_eq!(response.choices[0].message.tool_calls.len(), 1);
    }
}
