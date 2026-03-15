use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEnvelope {
    pub version: u16,
    pub task_id: String,
    pub turn: u32,
    pub seq: u64,
    pub event: RuntimeEvent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    TurnStart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<String>,
    },
    AssistantDelta {
        text: String,
    },
    AssistantMessage {
        content: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolResult {
        tool_call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        is_error: bool,
        output: String,
    },
    ApprovalRequest {
        capability: String,
        scope: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
    },
    ApprovalResolved {
        capability: String,
        scope: String,
        approved: bool,
    },
    ValidationResult {
        passed: bool,
        outputs: Vec<ValidationOutputEnvelope>,
    },
    TurnEnd {
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsageEnvelope>,
        changed_files: Vec<String>,
    },
    Error {
        code: String,
        message: String,
        recoverable: bool,
    },
    MaxTurnsReached {
        max_turns: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeRequest {
    SubmitInput {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        input: String,
    },
    Interrupt {
        task_id: String,
    },
    ApproveCapability {
        task_id: String,
        capability: String,
        scope: String,
    },
    DenyCapability {
        task_id: String,
        capability: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationOutputEnvelope {
    pub label: String,
    pub exit_code: i32,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TokenUsageEnvelope {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub estimated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_pi_09_anchor_runtime_envelope_serde_shape() {
        let envelope = RuntimeEnvelope {
            version: 1,
            task_id: "task-1741700000000".to_string(),
            turn: 1,
            seq: 3,
            event: RuntimeEvent::ToolCall {
                id: "call_1741700123456_9a2f".to_string(),
                name: "read_file".to_string(),
                arguments: json!({
                    "path": "src/app.rs"
                }),
            },
        };

        let value = serde_json::to_value(&envelope).expect("runtime envelope must serialize");
        assert_eq!(value["version"], 1);
        assert_eq!(value["task_id"], "task-1741700000000");
        assert_eq!(value["turn"], 1);
        assert_eq!(value["seq"], 3);
        assert_eq!(value["event"]["type"], "tool_call");
        assert_eq!(value["event"]["id"], "call_1741700123456_9a2f");
        assert_eq!(value["event"]["name"], "read_file");
        assert_eq!(value["event"]["arguments"]["path"], "src/app.rs");
    }

    #[test]
    fn test_pi_11_schema_assets_parse_as_json() {
        let envelope_schema: serde_json::Value =
            serde_json::from_str(include_str!("../../schemas/runtime_envelope_v1.json"))
                .expect("runtime envelope schema must parse");
        let request_schema: serde_json::Value =
            serde_json::from_str(include_str!("../../schemas/runtime_request_v1.json"))
                .expect("runtime request schema must parse");

        assert_eq!(
            envelope_schema["$id"],
            "https://vexcoder.io/schemas/runtime_envelope_v1.json"
        );
        assert_eq!(
            request_schema["$id"],
            "https://vexcoder.io/schemas/runtime_request_v1.json"
        );
        assert_eq!(envelope_schema["properties"]["version"]["const"], 1);
        assert_eq!(request_schema["$defs"]["scope"]["enum"][0], "once");
        assert_eq!(request_schema["$defs"]["scope"]["enum"][1], "session");
    }

    #[test]
    fn test_pi_11_tool_call_grammar_keeps_mcp_namespace_rule() {
        let grammar = include_str!("../../grammars/tool_call.gbnf");

        assert!(grammar.contains("tool_call ::= \"{\""));
        assert!(grammar.contains("mcp_tool ::= \"\\\"mcp."));
        assert!(grammar.contains("\\\"read_file\\\""));
        assert!(grammar.contains("\\\"apply_patch\\\""));
    }
}
