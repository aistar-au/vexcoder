use super::super::history::truncate_for_history;
use super::super::send_message::emit_tool_error;

#[test]
fn test_emit_tool_error_text_protocol_truncates_rendered_payload_once() {
    let clarification = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let limit = 32;
    let mut tool_result_blocks = Vec::new();
    let mut text_protocol_tool_results = Vec::new();

    emit_tool_error(
        None,
        "read_file",
        clarification,
        "toolu_error_01".to_string(),
        false,
        limit,
        &mut tool_result_blocks,
        &mut text_protocol_tool_results,
    );

    let expected = truncate_for_history(&format!("tool_error read_file:\n{clarification}"), limit);
    let double_truncated = truncate_for_history(
        &format!(
            "tool_error read_file:\n{}",
            truncate_for_history(clarification, limit)
        ),
        limit,
    );

    assert!(tool_result_blocks.is_empty());
    assert_eq!(text_protocol_tool_results, vec![expected.clone()]);
    assert_ne!(expected, double_truncated);
}
