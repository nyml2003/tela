//! claude CLI stream-json 线格式的解析与构造（纯函数，零 IO）。
//!
//! 形状以 claude 2.1.245 实测为准（见 tests/fixtures/*.jsonl 与 docs/038）：
//! - 回合输入：stdin 写 `user_turn_line`；同一进程可连续多回合，`session_id` 不变。
//! - 回合输出：stdout 逐行 JSON，`type` 字段可能出现在对象尾部，解析按字段而非顺序。
//! - 权限：stdout 出 `control_request`（`subtype = "can_use_tool"`），stdin 回
//!   `control_response`（`request_id` 嵌在 `response` 里——官方 SDK 的确切包络）。

use serde_json::Value;

/// 助手消息的一个内容块。
#[derive(Clone, Debug, PartialEq)]
pub enum ClaudeBlock {
    /// 文本块（thinking 块刻意不转发，但同消息的文本累积语义保持完整）。
    Text(String),
    /// 工具调用块。
    ToolUse {
        tool_use_id: String,
        tool_name: String,
        /// `input` 对象的紧凑 JSON 串（异构端不共享结构）。
        input_json: String,
    },
}

/// 一行 stdout 消息解析后的语义。
#[derive(Clone, Debug, PartialEq)]
pub enum ClaudeLine {
    /// `system/init`：回合开始，携带真实 session id。
    Init { session_id: String },
    /// `assistant` 消息的一个内容块；同一 `message.id` 可能分多行输出多个块。
    AssistantBlock {
        message_id: String,
        block: ClaudeBlock,
    },
    /// `user` 消息内的工具结果。
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    /// `result`：回合结束。
    TurnFinished {
        subtype: String,
        session_id: Option<String>,
        cost_usd: Option<f64>,
        duration_ms: Option<u64>,
    },
    /// `control_request`（`can_use_tool`）：CLI 请求权限裁决。
    ControlRequest {
        request_id: String,
        tool_use_id: Option<String>,
        tool_name: String,
        /// `request.input` 的紧凑 JSON 串。
        input_json: String,
    },
    /// 可安全忽略的消息（thinking_tokens、permission_denied、未知类型）。
    Ignore,
}

/// 解析一行 stdout；非 JSON、空行与已知噪音返回空向量。
///
/// 一行可能产出多个语义（如 `assistant` 行的 content 数组含多个块），因此返回向量。
pub fn parse_line(line: &str) -> Vec<ClaudeLine> {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.starts_with('{') {
        return Vec::new();
    }
    let value: Value = match serde_json::from_str(trimmed) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    match value["type"].as_str().unwrap_or_default() {
        "system" => parse_system(&value),
        "assistant" => parse_assistant(&value),
        "user" => parse_user(&value),
        "result" => vec![parse_result(&value)],
        "control_request" => parse_control_request(&value),
        _ => Vec::new(),
    }
}

fn parse_system(value: &Value) -> Vec<ClaudeLine> {
    match value["subtype"].as_str().unwrap_or_default() {
        "init" => match value["session_id"].as_str() {
            Some(session_id) => vec![ClaudeLine::Init {
                session_id: session_id.to_owned(),
            }],
            None => Vec::new(),
        },
        // permission_denied 由后续 tool_result 表达；thinking_tokens 是节奏噪音。
        _ => vec![ClaudeLine::Ignore],
    }
}

fn parse_assistant(value: &Value) -> Vec<ClaudeLine> {
    let Some(message_id) = value["message"]["id"].as_str() else {
        return vec![ClaudeLine::Ignore];
    };
    let mut lines = Vec::new();
    if let Some(blocks) = value["message"]["content"].as_array() {
        for block in blocks {
            match block["type"].as_str().unwrap_or_default() {
                "text" => {
                    if let Some(text) = block["text"].as_str() {
                        lines.push(ClaudeLine::AssistantBlock {
                            message_id: message_id.to_owned(),
                            block: ClaudeBlock::Text(text.to_owned()),
                        });
                    }
                }
                "tool_use" => {
                    if let (Some(tool_use_id), Some(tool_name)) =
                        (block["id"].as_str(), block["name"].as_str())
                    {
                        lines.push(ClaudeLine::AssistantBlock {
                            message_id: message_id.to_owned(),
                            block: ClaudeBlock::ToolUse {
                                tool_use_id: tool_use_id.to_owned(),
                                tool_name: tool_name.to_owned(),
                                input_json: compact_json(&block["input"]),
                            },
                        });
                    }
                }
                _ => {}
            }
        }
    }
    if lines.is_empty() {
        lines.push(ClaudeLine::Ignore);
    }
    lines
}

fn parse_user(value: &Value) -> Vec<ClaudeLine> {
    let mut lines = Vec::new();
    if let Some(blocks) = value["message"]["content"].as_array() {
        for block in blocks {
            if block["type"].as_str() == Some("tool_result")
                && let Some(tool_use_id) = block["tool_use_id"].as_str()
            {
                lines.push(ClaudeLine::ToolResult {
                    tool_use_id: tool_use_id.to_owned(),
                    content: result_content(block),
                    is_error: block["is_error"].as_bool().unwrap_or(false),
                });
            }
        }
    }
    if lines.is_empty() {
        lines.push(ClaudeLine::Ignore);
    }
    lines
}

/// `tool_result.content` 既可能是字符串也可能是块数组；统一拍平成文本。
fn result_content(block: &Value) -> String {
    match &block["content"] {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => {
            let mut parts = Vec::new();
            for part in blocks {
                match part["text"].as_str() {
                    Some(text) => parts.push(text.to_owned()),
                    None => {
                        if let Some(image) = part["source"]["data"].as_str() {
                            parts.push(format!("[image {} bytes]", image.len()));
                        }
                    }
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

fn parse_result(value: &Value) -> ClaudeLine {
    ClaudeLine::TurnFinished {
        subtype: value["subtype"].as_str().unwrap_or("unknown").to_owned(),
        session_id: value["session_id"].as_str().map(str::to_owned),
        cost_usd: value["total_cost_usd"].as_f64(),
        duration_ms: value["duration_ms"].as_u64(),
    }
}

fn parse_control_request(value: &Value) -> Vec<ClaudeLine> {
    if value["request"]["subtype"].as_str() != Some("can_use_tool") {
        return vec![ClaudeLine::Ignore];
    }
    let Some(request_id) = value["request_id"].as_str() else {
        return vec![ClaudeLine::Ignore];
    };
    vec![ClaudeLine::ControlRequest {
        request_id: request_id.to_owned(),
        tool_use_id: value["request"]["tool_use_id"].as_str().map(str::to_owned),
        tool_name: value["request"]["tool_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned(),
        input_json: compact_json(&value["request"]["input"]),
    }]
}

fn compact_json(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// 构造注入一回合用户输入的 stdin 行。
pub fn user_turn_line(text: &str) -> String {
    let value = serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{ "type": "text", "text": text }],
        },
    });
    serde_json::to_string(&value).expect("serialize user turn")
}

/// 构造权限应答的 stdin 行（`request_id` 嵌在 `response` 内，对齐官方 SDK 包络）。
pub fn permission_response_line(
    request_id: &str,
    tool_use_id: Option<&str>,
    allow: bool,
    message: &str,
) -> String {
    let mut decision = serde_json::Map::new();
    decision.insert(
        "behavior".to_owned(),
        Value::String(if allow { "allow" } else { "deny" }.to_owned()),
    );
    if let Some(tool_use_id) = tool_use_id {
        decision.insert(
            "toolUseID".to_owned(),
            Value::String(tool_use_id.to_owned()),
        );
    }
    if !allow && !message.is_empty() {
        decision.insert("message".to_owned(), Value::String(message.to_owned()));
    }
    let value = serde_json::json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": decision,
        },
    });
    serde_json::to_string(&value).expect("serialize permission response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_turn_line_matches_the_stream_json_shape() {
        let line = user_turn_line("看一下当前项目");
        let value: Value = serde_json::from_str(&line).expect("parse");
        assert_eq!(value["type"], "user");
        assert_eq!(value["message"]["content"][0]["text"], "看一下当前项目");
    }

    #[test]
    fn permission_response_lines_match_the_sdk_envelope() {
        let allow = permission_response_line("req-1", Some("call-1"), true, "");
        let value: Value = serde_json::from_str(&allow).expect("parse");
        assert_eq!(value["type"], "control_response");
        assert_eq!(value["response"]["subtype"], "success");
        assert_eq!(value["response"]["request_id"], "req-1");
        assert_eq!(value["response"]["response"]["behavior"], "allow");
        assert_eq!(value["response"]["response"]["toolUseID"], "call-1");

        let deny = permission_response_line("req-2", None, false, "用户拒绝");
        let value: Value = serde_json::from_str(&deny).expect("parse");
        assert_eq!(value["response"]["response"]["behavior"], "deny");
        assert_eq!(value["response"]["response"]["message"], "用户拒绝");
        assert!(value["response"]["response"].get("toolUseID").is_none());
    }

    #[test]
    fn non_json_and_noise_lines_are_ignored() {
        assert!(parse_line("").is_empty());
        assert!(parse_line("   ").is_empty());
        assert!(parse_line("not json at all").is_empty());
        assert_eq!(
            parse_line(r#"{"type":"system","subtype":"thinking_tokens","estimated_tokens":3}"#),
            vec![ClaudeLine::Ignore]
        );
        assert_eq!(
            parse_line(
                r#"{"type":"system","subtype":"permission_denied","tool_name":"Bash","tool_use_id":"c1","message":"blocked"}"#
            ),
            vec![ClaudeLine::Ignore]
        );
        assert_eq!(
            parse_line(r#"{"type":"stream_event","event":{}}"#),
            Vec::new()
        );
    }

    #[test]
    fn assistant_line_with_mixed_blocks_yields_one_line_per_block() {
        let raw = concat!(
            r#"{"type":"assistant","message":{"id":"msg_1","type":"message","role":"assistant","#,
            r#""content":[{"type":"text","text":"先看一下"},{"type":"tool_use","id":"call_9","#,
            r#""name":"Bash","input":{"command":"ls","description":"列目录"}}]},"session_id":"s"}"#
        );
        assert_eq!(
            parse_line(raw),
            vec![
                ClaudeLine::AssistantBlock {
                    message_id: "msg_1".to_owned(),
                    block: ClaudeBlock::Text("先看一下".to_owned()),
                },
                ClaudeLine::AssistantBlock {
                    message_id: "msg_1".to_owned(),
                    block: ClaudeBlock::ToolUse {
                        tool_use_id: "call_9".to_owned(),
                        tool_name: "Bash".to_owned(),
                        input_json: r#"{"command":"ls","description":"列目录"}"#.to_owned(),
                    },
                },
            ]
        );
    }

    #[test]
    fn result_line_parses_even_with_type_at_the_tail() {
        let raw = concat!(
            r#"{"is_error":false,"num_turns":2,"stop_reason":"end_turn","session_id":"8bf4a5a1","#,
            r#""total_cost_usd":0.028,"usage":{},"subtype":"success","result":"done","#,
            r#""duration_ms":4057,"type":"result"}"#
        );
        assert_eq!(
            parse_line(raw),
            vec![ClaudeLine::TurnFinished {
                subtype: "success".to_owned(),
                session_id: Some("8bf4a5a1".to_owned()),
                cost_usd: Some(0.028),
                duration_ms: Some(4057),
            }]
        );
    }

    #[test]
    fn control_request_line_parses_the_can_use_tool_shape() {
        let raw = concat!(
            r#"{"type":"control_request","request_id":"66ae0533","request":{"subtype":"can_use_tool","#,
            r#""tool_name":"Bash","display_name":"Bash","input":{"command":"echo forced > /tmp/p.txt"},"#,
            r#""description":"写入","permission_suggestions":[],"blocked_path":"/tmp/p.txt","#,
            r#""tool_use_id":"call_cec5"}}"#
        );
        assert_eq!(
            parse_line(raw),
            vec![ClaudeLine::ControlRequest {
                request_id: "66ae0533".to_owned(),
                tool_use_id: Some("call_cec5".to_owned()),
                tool_name: "Bash".to_owned(),
                input_json: r#"{"command":"echo forced > /tmp/p.txt"}"#.to_owned(),
            }]
        );
    }
}
