use super::{
    load_validate_toml, makefile_has_test_target, ValidationCommand, ValidationOutput,
    ValidationResult, ValidationSuite,
};
use crate::runtime::PassthroughSandbox;
use std::fs;

#[tokio::test]
async fn test_validation_suite_formats_failure_for_retry() {
    let suite = ValidationSuite { commands: vec![] };
    let result = ValidationResult {
        passed: false,
        outputs: vec![ValidationOutput {
            label: "cargo test".to_string(),
            exit_code: 101,
            stdout_tail: String::new(),
            stderr_tail: "assertion failed".to_string(),
            stdout_truncated: false,
            stderr_truncated: false,
        }],
    };

    let formatted = suite.format_for_retry(&result);
    assert!(formatted.contains("cargo test"));
    assert!(formatted.contains("assertion failed"));
}

#[test]
fn test_validation_suite_infers_rust_and_node_when_both_present() {
    let workspace = tempfile::tempdir().expect("tempdir");
    fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname=\"x\"\n",
    )
    .expect("write Cargo.toml");
    fs::write(workspace.path().join("package.json"), "{\"name\":\"x\"}")
        .expect("write package.json");

    let suite = ValidationSuite::infer_from_repo(workspace.path());
    let labels: Vec<String> = suite
        .commands
        .iter()
        .map(|command| command.label.to_ascii_lowercase())
        .collect();

    assert!(suite.commands.len() >= 2);
    assert!(
        labels.iter().any(|label| label.contains("cargo")),
        "expected at least one cargo command"
    );
    assert!(
        labels.iter().any(|label| label.contains("npm")),
        "expected at least one npm command"
    );
}

#[test]
fn test_validate_toml_single_quoted_label_parses() {
    let raw = "[[commands]]\nlabel = 'cargo test'\nprogram = 'cargo'\nargs = [\"test\"]\n";
    let result = load_validate_toml(raw);
    assert!(result.is_ok());
    let commands = result.expect("parse validate.toml");
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].program, "cargo");
    assert_eq!(commands[0].args, vec!["test".to_string()]);
}

#[test]
fn test_validate_toml_invalid_content_returns_error() {
    let raw = "[[commands]]\nlabel = 'cargo test'\nprogram = 'cargo'\nargs = [\"test\"\n";
    let result = load_validate_toml(raw);
    assert!(result.is_err(), "malformed validate.toml must error");
}

#[test]
fn test_load_or_infer_falls_back_on_invalid_validate_toml() {
    let workspace = tempfile::tempdir().expect("tempdir");
    fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname='x'\nversion='0.1.0'\n",
    )
    .expect("write Cargo.toml");
    fs::create_dir_all(workspace.path().join(".vex")).expect("mkdir .vex");
    fs::write(
        workspace.path().join(".vex/validate.toml"),
        "[[commands]]\nlabel='broken'\nprogram='cargo'\nargs=[\"test\"\n",
    )
    .expect("write invalid validate.toml");

    let suite = ValidationSuite::load_or_infer(workspace.path());
    assert!(
        suite
            .commands
            .iter()
            .any(|command| command.label == "cargo check"),
        "must fall back to inferred suite when validate.toml parse fails"
    );
}

#[test]
fn test_makefile_target_detection_rejects_indented_test_target() {
    let workspace = tempfile::tempdir().expect("tempdir");
    fs::write(
        workspace.path().join("Makefile"),
        "all:\n\t@echo test: indented\n",
    )
    .expect("write Makefile");
    assert!(
        !makefile_has_test_target(workspace.path()),
        "indented test: must not be detected as a make target"
    );
}

#[test]
fn test_makefile_target_detection_accepts_column_zero_test_target() {
    let workspace = tempfile::tempdir().expect("tempdir");
    fs::write(workspace.path().join("Makefile"), "test:\n\tcargo test\n").expect("write Makefile");
    assert!(
        makefile_has_test_target(workspace.path()),
        "column-zero test: must be detected"
    );
}

#[tokio::test]
async fn test_validation_suite_empty_suite_exits_on_clean_patch() {
    use crate::runtime::command::DefaultCommandRunner;

    let suite = ValidationSuite::default();
    let runner = DefaultCommandRunner::new();
    let result = suite
        .run(&runner)
        .await
        .expect("empty suite must not error");

    assert!(
        result.passed,
        "empty validation suite must report passed=true"
    );
    assert!(
        result.outputs.is_empty(),
        "empty validation suite must produce no outputs"
    );
}

#[tokio::test]
async fn test_validation_suite_run_in_dir_uses_requested_working_dir() {
    use crate::runtime::command::DefaultCommandRunner;

    let workspace = tempfile::tempdir().expect("tempdir");
    let suite = ValidationSuite {
        commands: vec![ValidationCommand {
            label: "print working dir".to_string(),
            #[cfg(windows)]
            program: "cmd".to_string(),
            #[cfg(not(windows))]
            program: "pwd".to_string(),
            #[cfg(windows)]
            args: vec!["/C".to_string(), "cd".to_string()],
            #[cfg(not(windows))]
            args: Vec::new(),
            timeout_secs: 2,
        }],
    };
    let runner = DefaultCommandRunner::new();

    let result = suite
        .run_in_dir(&runner, Some(workspace.path()))
        .await
        .expect("validation suite must run");

    assert!(result.passed);
    assert_eq!(result.outputs.len(), 1);
    assert!(
        result.outputs[0]
            .stdout_tail
            .contains(&workspace.path().display().to_string()),
        "validation command must run from requested working dir: {:?}",
        result.outputs[0]
    );
}

#[tokio::test]
async fn test_validation_suite_run_in_dir_with_sandbox_normalizes_zero_timeout() {
    use crate::runtime::command::DefaultCommandRunner;

    let workspace = tempfile::tempdir().expect("tempdir");
    let suite = ValidationSuite {
        commands: vec![ValidationCommand {
            label: "print working dir".to_string(),
            #[cfg(windows)]
            program: "cmd".to_string(),
            #[cfg(not(windows))]
            program: "pwd".to_string(),
            #[cfg(windows)]
            args: vec!["/C".to_string(), "cd".to_string()],
            #[cfg(not(windows))]
            args: Vec::new(),
            timeout_secs: 0,
        }],
    };
    let runner = DefaultCommandRunner::new();

    let result = suite
        .run_in_dir_with_sandbox(&runner, &PassthroughSandbox, Some(workspace.path()))
        .await
        .expect("sandboxed validation suite must run");

    assert!(
        result.passed,
        "zero timeout should normalize to the default"
    );
    assert_eq!(result.outputs.len(), 1);
    assert!(
        result.outputs[0]
            .stdout_tail
            .contains(&workspace.path().display().to_string()),
        "sandboxed validation command must run from requested working dir: {:?}",
        result.outputs[0]
    );
}
