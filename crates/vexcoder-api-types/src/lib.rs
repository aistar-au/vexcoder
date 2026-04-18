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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        citations: Option<Vec<serde_json::Value>>,
    },
    ToolUse {
        id: String,
        name: String,
        #[serde(default = "default_json_object")]
        input: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<ToolUseMetadata>,
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
    #[serde(rename = "redacted_thinking")]
    WithheldThinking {
        data: String,
    },
    ServerToolUse {
        id: String,
        name: String,
        #[serde(default = "default_json_object")]
        input: serde_json::Value,
    },
    WebSearchToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: serde_json::Value,
    },
}

fn default_json_object() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamChunkMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_progress: Option<StreamPromptProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timings: Option<StreamTimings>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StreamPromptProgress {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_ms: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StreamTimings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_n: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_n: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_per_token_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_per_second: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_n: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_per_token_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_per_second: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolUseMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice_index: Option<usize>,
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
        #[serde(default)]
        usage: Option<ApiUsage>,
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
    #[serde(default)]
    pub choice_index: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageStartData {
    pub id: String,
    #[serde(rename = "type", default)]
    pub message_type: Option<String>,
    pub role: String,
    pub model: String,
    #[serde(default)]
    pub content: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
    #[serde(default)]
    pub usage: Option<ApiUsage>,
    #[serde(default)]
    pub metadata: Option<StreamChunkMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageDelta {
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub refusal: Option<String>,
    #[serde(default)]
    pub metadata: Option<StreamChunkMetadata>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiUsage {
    // Core token counts (cross-protocol normalised)
    #[serde(default, alias = "prompt_tokens")]
    pub input_tokens: Option<u64>,
    #[serde(default, alias = "completion_tokens")]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub total_tokens: Option<u64>,
    // Provider cache fields
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation: Option<serde_json::Value>,
    // Extended usage metadata
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(default)]
    pub web_search_requests: Option<u64>,
    #[serde(default)]
    pub inference_geo: Option<String>,
    // Detailed token breakdowns (chat completions)
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokenDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokenDetails>,
}

/// Prompt token detail breakdown (chat completions protocol).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PromptTokenDetails {
    #[serde(default)]
    pub cached_tokens: Option<u64>,
    #[serde(default)]
    pub audio_tokens: Option<u64>,
}

/// Completion token detail breakdown (chat completions protocol).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CompletionTokenDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
    #[serde(default)]
    pub audio_tokens: Option<u64>,
    #[serde(default)]
    pub accepted_prediction_tokens: Option<u64>,
    #[serde(default)]
    pub rejected_prediction_tokens: Option<u64>,
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
    fn test_content_block_withheld_thinking_deserialises() {
        let json = r#"{"type":"redacted_thinking","data":"opaque"}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        assert!(matches!(block, ContentBlock::WithheldThinking { .. }));
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
        let json = r#"{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":200,"cache_read_input_tokens":800,"inference_geo":"au"}"#;
        let usage: ApiUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(50));
        assert_eq!(usage.cache_creation_input_tokens, Some(200));
        assert_eq!(usage.cache_read_input_tokens, Some(800));
        assert_eq!(usage.inference_geo.as_deref(), Some("au"));
        assert!(usage.total_tokens.is_none());
    }

    #[test]
    fn test_api_usage_chat_compat_fields_deserialise() {
        let json = r#"{"prompt_tokens":120,"completion_tokens":30,"total_tokens":150}"#;
        let usage: ApiUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(30));
        assert_eq!(usage.total_tokens, Some(150));
        assert!(usage.cache_creation_input_tokens.is_none());
        assert!(usage.cache_read_input_tokens.is_none());
    }

    #[test]
    fn test_message_delta_stop_sequence_deserialises() {
        let json = r#"{"stop_reason":"stop_sequence","stop_sequence":"\n\nHuman:","role":"assistant","refusal":"no","metadata":{"choice_index":2,"logprobs":{"content":[]}}}"#;
        let delta: MessageDelta = serde_json::from_str(json).unwrap();
        assert_eq!(delta.stop_reason.as_deref(), Some("stop_sequence"));
        assert_eq!(delta.stop_sequence.as_deref(), Some("\n\nHuman:"));
        assert_eq!(delta.role.as_deref(), Some("assistant"));
        assert_eq!(delta.refusal.as_deref(), Some("no"));
        let metadata = delta.metadata.expect("metadata should be present");
        assert_eq!(metadata.choice_index, Some(2));
        assert!(metadata.logprobs.is_some());
    }

    #[test]
    fn test_message_delta_event_top_level_usage_deserialises() {
        // Messages v1 wire format: usage is a peer of delta, not nested inside it
        let json = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":15}}"#;
        let evt: StreamEvent = serde_json::from_str(json).unwrap();
        match evt {
            StreamEvent::MessageDelta { delta, usage } => {
                assert_eq!(delta.stop_reason.as_deref(), Some("end_turn"));
                let usage = usage.expect("top-level usage must be present");
                assert_eq!(usage.output_tokens, Some(15));
            }
            other => panic!("expected MessageDelta variant, got {:?}", other),
        }
    }

    #[test]
    fn test_message_start_full_message_deserialises() {
        let json = r#"{
            "type":"message_start",
            "message":{
                "id":"msg_01XY","type":"message","role":"assistant",
                "content":[],"model":"model-name-20241022",
                "stop_reason":null,"stop_sequence":null,
                "usage":{"input_tokens":25,"output_tokens":1},
                "metadata":{"object":"chat.completion.chunk","created":1741730100,"system_fingerprint":"fp_123","service_tier":"standard"}
            }
        }"#;
        let evt: StreamEvent = serde_json::from_str(json).unwrap();
        match evt {
            StreamEvent::MessageStart { message } => {
                assert_eq!(message.id, "msg_01XY");
                assert_eq!(message.message_type.as_deref(), Some("message"));
                assert_eq!(message.role, "assistant");
                assert_eq!(message.model, "model-name-20241022");
                assert!(message.stop_reason.is_none());
                assert!(message.stop_sequence.is_none());
                let content = message.content.expect("content array must be present");
                assert!(content.is_empty());
                let usage = message.usage.expect("usage must be present");
                assert_eq!(usage.input_tokens, Some(25));
                assert_eq!(usage.output_tokens, Some(1));
                let metadata = message.metadata.expect("metadata must be present");
                assert_eq!(metadata.object.as_deref(), Some("chat.completion.chunk"));
                assert_eq!(metadata.created, Some(1741730100));
                assert_eq!(metadata.system_fingerprint.as_deref(), Some("fp_123"));
                assert_eq!(metadata.service_tier.as_deref(), Some("standard"));
            }
            other => panic!("expected MessageStart variant, got {:?}", other),
        }
    }

    #[test]
    fn test_content_block_text_with_citations_deserialises() {
        let json = r#"{"type":"text","text":"hello","citations":[{"type":"char_location","start":0,"end":5}]}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        match block {
            ContentBlock::Text { text, citations } => {
                assert_eq!(text, "hello");
                let cites = citations.expect("citations must be present");
                assert_eq!(cites.len(), 1);
            }
            other => panic!("expected Text variant, got {:?}", other),
        }
    }

    #[test]
    fn test_content_block_server_tool_use_deserialises() {
        let json = r#"{"type":"server_tool_use","id":"srvtoolu_01","name":"web_search","input":{"query":"test"}}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        match block {
            ContentBlock::ServerToolUse { id, name, input } => {
                assert_eq!(id, "srvtoolu_01");
                assert_eq!(name, "web_search");
                assert_eq!(input["query"], "test");
            }
            other => panic!("expected ServerToolUse variant, got {:?}", other),
        }
    }

    #[test]
    fn test_content_block_web_search_tool_result_deserialises() {
        let json = r#"{"type":"web_search_tool_result","tool_use_id":"srvtoolu_01","content":[{"type":"web_search_result","url":"https://example.com","title":"Example"}]}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        match block {
            ContentBlock::WebSearchToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "srvtoolu_01");
                assert!(content.is_array());
            }
            other => panic!("expected WebSearchToolResult variant, got {:?}", other),
        }
    }

    #[test]
    fn test_api_usage_extended_fields_deserialise() {
        let json = r#"{
            "input_tokens":100,"output_tokens":50,
            "cache_creation_input_tokens":200,"cache_read_input_tokens":800,
            "service_tier":"standard","web_search_requests":3,"inference_geo":"au",
            "cache_creation":{"type":"ephemeral","duration":"5m"}
        }"#;
        let usage: ApiUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.service_tier.as_deref(), Some("standard"));
        assert_eq!(usage.web_search_requests, Some(3));
        assert_eq!(usage.inference_geo.as_deref(), Some("au"));
        assert!(usage.cache_creation.is_some());
    }

    #[test]
    fn test_api_usage_chat_compat_detail_breakdowns_deserialise() {
        let json = r#"{
            "prompt_tokens":120,"completion_tokens":30,"total_tokens":150,
            "prompt_tokens_details":{"cached_tokens":80,"audio_tokens":0},
            "completion_tokens_details":{"reasoning_tokens":10,"audio_tokens":0,"accepted_prediction_tokens":5,"rejected_prediction_tokens":2}
        }"#;
        let usage: ApiUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(30));
        assert_eq!(usage.total_tokens, Some(150));
        let prompt_details = usage.prompt_tokens_details.expect("prompt details");
        assert_eq!(prompt_details.cached_tokens, Some(80));
        let completion_details = usage.completion_tokens_details.expect("completion details");
        assert_eq!(completion_details.reasoning_tokens, Some(10));
        assert_eq!(completion_details.accepted_prediction_tokens, Some(5));
        assert_eq!(completion_details.rejected_prediction_tokens, Some(2));
    }

    #[test]
    fn test_tool_use_metadata_serialises_when_present() {
        let block = ContentBlock::ToolUse {
            id: "call_abc".into(),
            name: "read_file".into(),
            input: serde_json::json!({"path":"src/lib.rs"}),
            metadata: Some(ToolUseMetadata {
                call_type: Some("function".into()),
                choice_index: Some(1),
            }),
        };

        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["metadata"]["call_type"], "function");
        assert_eq!(json["metadata"]["choice_index"], 1);
    }
}
