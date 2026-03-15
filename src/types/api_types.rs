use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: String,
    pub content: Content,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default = "default_json_object")]
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
}

fn default_json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        message: MessageStartData,
    },
    ContentBlockStart {
        index: usize,
        content_block: ContentBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: Delta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: MessageDelta,
    },
    MessageStop,
    Ping,
    Error {
        error: ApiStreamError,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Delta {
    #[serde(rename = "type")]
    #[serde(default)]
    pub delta_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub partial_json: Option<String>,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageStartData {
    pub id: String,
    pub role: String,
    pub model: String,
    #[serde(default)]
    pub usage: Option<ApiUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageDelta {
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
    #[serde(default)]
    pub usage: Option<ApiUsage>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_write_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiStreamError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crit_02_regression() {
        let msg = ApiMessage {
            role: "user".into(),
            content: Content::Text("Hello".into()),
        };
        let serialized = serde_json::to_value(&msg).unwrap();

        // ANCHOR: This assertion will FAIL if #[serde(flatten)] is present
        // because the "content" key will be missing from the object.
        assert!(
            serialized.get("content").is_some(),
            "Missing 'content' key in JSON!"
        );
    }

    #[test]
    fn test_stream_event_error_deserialises() {
        let json = r#"{"type":"error","error":{"type":"overloaded_error","message":"overloaded"}}"#;
        let evt: StreamEvent = serde_json::from_str(json).unwrap();
        match evt {
            StreamEvent::Error { error } => {
                assert_eq!(error.error_type, "overloaded_error");
                assert_eq!(error.message, "overloaded");
            }
            other => panic!("expected Error variant, got {:?}", other),
        }
    }

    #[test]
    fn test_stream_event_ping_deserialises() {
        let json = r#"{"type":"ping"}"#;
        let evt: StreamEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(evt, StreamEvent::Ping));
    }

    #[test]
    fn test_content_block_thinking_deserialises() {
        let json = r#"{"type":"thinking","thinking":"let me think","signature":"sig123"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        match block {
            ContentBlock::Thinking {
                thinking,
                signature,
            } => {
                assert_eq!(thinking, "let me think");
                assert_eq!(signature, "sig123");
            }
            other => panic!("expected Thinking variant, got {:?}", other),
        }
    }

    #[test]
    fn test_content_block_redacted_thinking_deserialises() {
        let json = r#"{"type":"redacted_thinking","data":"opaque"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        assert!(matches!(block, ContentBlock::RedactedThinking { .. }));
    }

    #[test]
    fn test_delta_thinking_fields_deserialise() {
        let json = r#"{"type":"thinking_delta","thinking":"partial thought"}"#;
        let delta: Delta = serde_json::from_str(json).unwrap();
        assert_eq!(delta.delta_type.as_deref(), Some("thinking_delta"));
        assert_eq!(delta.thinking.as_deref(), Some("partial thought"));
        assert!(delta.signature.is_none());
    }

    #[test]
    fn test_api_usage_cache_fields_deserialise() {
        let json = r#"{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":200,"cache_read_input_tokens":800}"#;
        let usage: ApiUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.cache_creation_input_tokens, Some(200));
        assert_eq!(usage.cache_read_input_tokens, Some(800));
        assert!(usage.cache_write_input_tokens.is_none());
    }

    #[test]
    fn test_message_delta_stop_sequence_deserialises() {
        let json = r#"{"stop_reason":"stop_sequence","stop_sequence":"\n\nHuman:"}"#;
        let delta: MessageDelta = serde_json::from_str(json).unwrap();
        assert_eq!(delta.stop_reason.as_deref(), Some("stop_sequence"));
        assert_eq!(delta.stop_sequence.as_deref(), Some("\n\nHuman:"));
    }
}
