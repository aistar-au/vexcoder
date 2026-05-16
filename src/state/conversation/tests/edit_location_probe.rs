use super::*;
use crate::config::SearchConfig;
use crate::tools::pulse_ledger::{
    clear_pulse_ledger, find_pulse_read, lock_pulse_ledger_for_tests,
};
use std::fs;

fn anchor_fixture() -> String {
    let mut buf = String::new();
    buf.push_str("fn first() {\n    let value = 1;\n}\n");
    for index in 0..5 {
        buf.push_str(&format!(
            "fn padding_top_{index}() {{\n    let unrelated = {index};\n}}\n"
        ));
    }
    buf.push_str("fn second() {\n    let value = 1;\n}\n");
    for index in 0..5 {
        buf.push_str(&format!(
            "fn padding_bottom_{index}() {{\n    let unrelated = {index};\n}}\n"
        ));
    }
    buf.push_str("fn third() {\n    let value = 1;\n}\n");
    buf
}

fn seed_workspace(contents: &str) -> (TempDir, std::path::PathBuf) {
    let workspace = TempDir::new().expect("tempdir");
    let path = workspace.path().join("module.rs");
    fs::write(&path, contents).expect("seed fixture");
    (workspace, path)
}

fn second_block_offset(fixture: &str) -> usize {
    fixture
        .lines()
        .position(|line| line == "fn second() {")
        .map(|index| index + 1)
        .expect("fixture must contain fn second")
}

#[test]
fn probe_a_anchored_match_resolves_ambiguous_edit() {
    let _lock = lock_pulse_ledger_for_tests();
    clear_pulse_ledger();

    let fixture = anchor_fixture();
    let (workspace, file_path) = seed_workspace(&fixture);
    let operator = ToolOperator::new(workspace.path().to_path_buf());
    let search_config = SearchConfig::default();

    let offset = second_block_offset(&fixture);
    let read_output = call_tool_routing_with_config(
        &operator,
        "read_file",
        &json!({"path": "module.rs", "offset": offset, "limit": 3}),
        &search_config,
    )
    .expect("read_file should succeed");
    assert!(read_output.contains("fn second"));

    let recorded = find_pulse_read(&file_path).expect("ledger must record the read");
    assert_eq!(recorded.start_line, offset);

    operator
        .edit_file("module.rs", "    let value = 1;", "    let value = 99;")
        .expect("anchored edit should resolve uniquely inside the read window");

    let edited = fs::read_to_string(&file_path).expect("read edited file");
    assert!(
        edited.contains("fn second() {\n    let value = 99;\n}"),
        "the edit must land on the in-window occurrence"
    );
    assert!(
        edited.contains("fn first() {\n    let value = 1;\n}"),
        "first occurrence (out of window) must be untouched"
    );
    assert!(
        edited.contains("fn third() {\n    let value = 1;\n}"),
        "third occurrence (out of window) must be untouched"
    );

    clear_pulse_ledger();
}

#[test]
fn probe_b_ledger_invalidates_after_external_mutation() {
    use filetime::{FileTime, set_file_mtime};

    let _lock = lock_pulse_ledger_for_tests();
    clear_pulse_ledger();

    let fixture = anchor_fixture();
    let (workspace, file_path) = seed_workspace(&fixture);
    set_file_mtime(&file_path, FileTime::from_unix_time(1, 0)).expect("set initial mtime");
    let operator = ToolOperator::new(workspace.path().to_path_buf());
    let search_config = SearchConfig::default();

    let offset = second_block_offset(&fixture);
    call_tool_routing_with_config(
        &operator,
        "read_file",
        &json!({"path": "module.rs", "offset": offset, "limit": 3}),
        &search_config,
    )
    .expect("read_file should succeed");
    assert!(find_pulse_read(&file_path).is_some());

    let mutated = format!("// drift inserted before original content\n{fixture}");
    fs::write(&file_path, mutated).expect("rewrite fixture");
    set_file_mtime(&file_path, FileTime::from_unix_time(2, 0)).expect("bump mtime");

    assert!(
        find_pulse_read(&file_path).is_none(),
        "external mutation must invalidate the ledger entry by fingerprint"
    );

    let error = operator
        .edit_file("module.rs", "    let value = 1;", "    let value = 99;")
        .expect_err(
            "ambiguous edit must fall back to the uniqueness rule once the anchor is stale",
        );
    let message = format!("{error:#}");
    assert!(
        message.contains("appears") && message.contains("must be unique"),
        "fallback path must surface the existing uniqueness error, got: {message}"
    );

    clear_pulse_ledger();
}

#[test]
fn probe_c_ledger_clears_between_pulses() {
    let _lock = lock_pulse_ledger_for_tests();
    clear_pulse_ledger();

    let fixture = anchor_fixture();
    let (workspace, file_path) = seed_workspace(&fixture);
    let operator = ToolOperator::new(workspace.path().to_path_buf());
    let search_config = SearchConfig::default();

    let offset = second_block_offset(&fixture);
    call_tool_routing_with_config(
        &operator,
        "read_file",
        &json!({"path": "module.rs", "offset": offset, "limit": 3}),
        &search_config,
    )
    .expect("read_file should succeed");
    assert!(find_pulse_read(&file_path).is_some());

    clear_pulse_ledger();

    assert!(
        find_pulse_read(&file_path).is_none(),
        "clearing the ledger between pulses must remove every entry"
    );

    let error = operator
        .edit_file("module.rs", "    let value = 1;", "    let value = 99;")
        .expect_err(
            "with no ledger entry the ambiguous edit must fall back to the uniqueness rule",
        );
    let message = format!("{error:#}");
    assert!(
        message.contains("appears") && message.contains("must be unique"),
        "fallback path must surface the existing uniqueness error, got: {message}"
    );

    clear_pulse_ledger();
}
