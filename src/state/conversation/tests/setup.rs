use super::*;

#[test]
fn test_conversation_module_structure() {
    let _ = std::any::TypeId::of::<ConversationManager>();
    let _ = std::any::TypeId::of::<ConversationStreamUpdate>();
    let _ = std::any::TypeId::of::<ToolApprovalRequest>();

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    assert!(
        manifest_dir
            .join("src/state/conversation/state.rs")
            .exists()
    );
    assert!(manifest_dir.join("src/state/conversation/core.rs").exists());
    assert!(
        manifest_dir
            .join("src/state/conversation/send_message.rs")
            .exists()
    );
    assert!(manifest_dir.join("src/state/conversation/tools").is_dir());
    assert!(
        manifest_dir
            .join("src/state/conversation/tools/mod.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("src/state/conversation/tools/config.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("src/state/conversation/tools/dispatch.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("src/state/conversation/tools/formatting.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("src/state/conversation/tools/index.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("src/state/conversation/tools/validation.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("src/state/conversation/streaming.rs")
            .exists()
    );
    assert!(
        manifest_dir
            .join("src/state/conversation/history.rs")
            .exists()
    );
}

pub(super) fn tagged_read_file_round(message_id: &str) -> Vec<String> {
    vec![
        format!(
            r#"event: message_start
data: {{"type":"message_start","message":{{"id":"{message_id}","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{{"input_tokens":10,"output_tokens":1}}}}}}"#
        ),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#
            .to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will read it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>"}}"#
            .to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#
            .to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#
            .to_string(),
    ]
}

pub(super) fn tagged_duplicate_read_file_round(message_id: &str) -> Vec<String> {
    vec![
        format!(
            r#"event: message_start
data: {{"type":"message_start","message":{{"id":"{message_id}","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{{"input_tokens":10,"output_tokens":1}}}}}}"#
        ),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#
            .to_string(),
        r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will inspect it.\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>\n<function=read_file>\n<parameter=path>\nfile.txt\n</parameter>\n</function>"}}"#
            .to_string(),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#
            .to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#
            .to_string(),
    ]
}

pub(super) fn plain_text_round(message_id: &str, text: &str) -> Vec<String> {
    vec![
        format!(
            r#"event: message_start
data: {{"type":"message_start","message":{{"id":"{message_id}","type":"message","role":"assistant","model":"mock-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{{"input_tokens":10,"output_tokens":1}}}}}}"#
        ),
        r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#
            .to_string(),
        format!(
            r#"event: content_block_delta
data: {{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{text}"}}}}"#
        ),
        r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":9}}"#
            .to_string(),
        r#"event: message_stop
data: {"type":"message_stop"}"#
            .to_string(),
    ]
}
