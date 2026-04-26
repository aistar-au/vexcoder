use super::{
    ValidationCommand, ValidationOutput, ValidationResult, ValidationSuite, load_validate_toml,
    makefile_has_test_target,
};
use std::fs;

#[tokio::test]
async fn validation_suite_formats_failure_output_for_retry() {
    let suite = ValidationSuite { commands: vec![] };
    let result = ValidationResult {
        passed: false,
        outputs: vec![ValidationOutput {
            label: "cargo test".to_string(),
            exit_code: 101,
            stdout_tail: String::new(),
            stderr_tail: "assertion failed".to_string(),
            stdout_tail_limited: false,
            stderr_tail_limited: false,
        }],
    };
    let formatted = suite.format_for_retry(&result);
    assert!(formatted.contains("cargo test") && formatted.contains("assertion failed"));
}

#[test]
fn validation_suite_infers_rust_and_node_when_both_present() {
    let workspace = tempfile::tempdir().unwrap();
    fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname=\"x\"\n",
    )
    .unwrap();
    fs::write(workspace.path().join("package.json"), "{\"name\":\"x\"}").unwrap();
    let suite = ValidationSuite::infer_from_repo(workspace.path());
    let labels: Vec<_> = suite
        .commands
        .iter()
        .map(|c| c.label.to_ascii_lowercase())
        .collect();
    assert!(labels.iter().any(|l| l.contains("cargo")));
    assert!(labels.iter().any(|l| l.contains("npm")));
}

#[test]
fn load_validate_toml_parses_valid_and_rejects_invalid() {
    let valid = "[[commands]]\nlabel = 'cargo test'\nprogram = 'cargo'\nargs = [\"test\"]\n";
    let cmds = load_validate_toml(valid).expect("valid toml");
    assert_eq!(cmds[0].program, "cargo");

    let invalid = "[[commands]]\nlabel = 'x'\nprogram = 'cargo'\nargs = [\"test\"\n";
    assert!(load_validate_toml(invalid).is_err());
}

#[test]
fn makefile_test_target_detection_requires_column_zero() {
    let workspace = tempfile::tempdir().unwrap();
    fs::write(
        workspace.path().join("Makefile"),
        "all:\n\t@echo test: indented\n",
    )
    .unwrap();
    assert!(!makefile_has_test_target(workspace.path()));

    fs::write(workspace.path().join("Makefile"), "test:\n\tcargo test\n").unwrap();
    assert!(makefile_has_test_target(workspace.path()));
}

#[tokio::test]
async fn empty_validation_suite_reports_passed_with_no_outputs() {
    use crate::runtime::command::DefaultCommandRunner;
    let suite = ValidationSuite::default();
    let result = suite.run(&DefaultCommandRunner::new()).await.unwrap();
    assert!(result.passed && result.outputs.is_empty());
}
