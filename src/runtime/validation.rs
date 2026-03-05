use anyhow::Result;
use std::fs;
use std::path::Path;
use tokio::time::{timeout, Duration};

use crate::runtime::text_util::truncate_tail_bytes;
use crate::runtime::{CommandRequest, CommandRunner};

const DEFAULT_TIMEOUT_SECS: u64 = 60;
pub const VALIDATION_TAIL_BYTES: usize = 8_192;

fn default_timeout_secs() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

#[derive(Debug, Clone, Default)]
pub struct ValidationSuite {
    pub commands: Vec<ValidationCommand>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ValidationCommand {
    pub label: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub passed: bool,
    pub outputs: Vec<ValidationOutput>,
}

#[derive(Debug, Clone)]
pub struct ValidationOutput {
    pub label: String,
    pub exit_code: i32,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(serde::Deserialize)]
struct ValidateConfig {
    commands: Vec<ValidationCommand>,
}

impl ValidationSuite {
    /// Run all commands in the suite and collect results.
    ///
    /// Wired by EL-03 (`EditLoop::run` step 6) and EL-05 (`/run`, `/test`).
    pub async fn run<R>(&self, runner: &R) -> Result<ValidationResult>
    where
        R: CommandRunner + ?Sized,
    {
        let mut passed = true;
        let mut outputs = Vec::with_capacity(self.commands.len());

        for command in &self.commands {
            let output = run_validation_command(command, runner).await;
            if output.exit_code != 0 {
                passed = false;
            }
            outputs.push(output);
        }

        Ok(ValidationResult { passed, outputs })
    }

    /// Format a failed `ValidationResult` as a structured retry-context block.
    ///
    /// Wired by EL-03 (`EditLoop::run` step 7).
    pub fn format_for_retry(&self, result: &ValidationResult) -> String {
        if result.passed {
            return "[validation passed]".to_string();
        }

        let mut out = String::from("[validation failed]\n");
        for output in &result.outputs {
            if output.exit_code == 0 {
                continue;
            }

            out.push_str(&format!(
                "\n[command: {}]\nexit_code: {}\n",
                output.label, output.exit_code
            ));

            if !output.stdout_tail.is_empty() {
                if output.stdout_truncated {
                    out.push_str("[stdout truncated — showing last 8192 bytes]\n");
                }
                out.push_str("stdout:\n```text\n");
                out.push_str(&output.stdout_tail);
                if !output.stdout_tail.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("```\n");
            }

            if !output.stderr_tail.is_empty() {
                if output.stderr_truncated {
                    out.push_str("[stderr truncated — showing last 8192 bytes]\n");
                }
                out.push_str("stderr:\n```text\n");
                out.push_str(&output.stderr_tail);
                if !output.stderr_tail.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("```\n");
            }
        }

        out
    }

    /// Infer a validation suite from standard project files at `root`.
    ///
    /// Wired by EL-03 and EL-05 when no `.vex/validate.toml` is present.
    pub fn infer_from_repo(root: &Path) -> Self {
        let mut commands = Vec::new();
        let has_cargo = root.join("Cargo.toml").is_file();
        let has_package = root.join("package.json").is_file();

        if has_cargo {
            commands.push(ValidationCommand {
                label: "cargo check".to_string(),
                program: "cargo".to_string(),
                args: vec!["check".to_string()],
                timeout_secs: DEFAULT_TIMEOUT_SECS,
            });
            commands.push(ValidationCommand {
                label: "cargo test".to_string(),
                program: "cargo".to_string(),
                args: vec!["test".to_string()],
                timeout_secs: DEFAULT_TIMEOUT_SECS,
            });
        }

        if has_package {
            commands.push(ValidationCommand {
                label: "npm test".to_string(),
                program: "npm".to_string(),
                args: vec!["test".to_string()],
                timeout_secs: DEFAULT_TIMEOUT_SECS,
            });
        }

        if makefile_has_test_target(root) {
            commands.push(ValidationCommand {
                label: "make test".to_string(),
                program: "make".to_string(),
                args: vec!["test".to_string()],
                timeout_secs: DEFAULT_TIMEOUT_SECS,
            });
        }

        Self { commands }
    }

    /// Load from `.vex/validate.toml` if present and valid, otherwise fall back to inference.
    ///
    /// Wired by EL-03 and EL-05.
    pub fn load_or_infer(root: &Path) -> Self {
        let config_path = root.join(".vex/validate.toml");
        if config_path.is_file() {
            match fs::read_to_string(&config_path) {
                Ok(raw) => match load_validate_toml(&raw) {
                    Ok(commands) if !commands.is_empty() => return Self { commands },
                    Ok(_) => {
                        eprintln!(
                            "[validation] {} parsed but produced an empty command suite; falling back to inferred defaults",
                            config_path.display()
                        );
                    }
                    Err(err) => {
                        if !raw.trim().is_empty() {
                            eprintln!(
                                "[validation] failed to parse {}: {err:#}",
                                config_path.display()
                            );
                        }
                    }
                },
                Err(err) => {
                    eprintln!(
                        "[validation] failed to read {}: {err:#}",
                        config_path.display()
                    );
                }
            }
        }
        Self::infer_from_repo(root)
    }
}

async fn run_validation_command<R>(command: &ValidationCommand, runner: &R) -> ValidationOutput
where
    R: CommandRunner + ?Sized,
{
    let timeout_secs = normalize_timeout(command.timeout_secs);

    if command.program.trim().is_empty() {
        return ValidationOutput {
            label: command.label.clone(),
            exit_code: -1,
            stdout_tail: String::new(),
            stderr_tail: "validation command program cannot be empty".to_string(),
            stdout_truncated: false,
            stderr_truncated: false,
        };
    }

    let req = CommandRequest {
        program: command.program.clone(),
        args: command.args.clone(),
    };

    let result = timeout(Duration::from_secs(timeout_secs), runner.run_one_shot(req)).await;
    match result {
        Ok(Ok(output)) => {
            let (stdout_tail, stdout_truncated) =
                truncate_tail_bytes(&output.stdout, VALIDATION_TAIL_BYTES);
            let (stderr_tail, stderr_truncated) =
                truncate_tail_bytes(&output.stderr, VALIDATION_TAIL_BYTES);
            ValidationOutput {
                label: command.label.clone(),
                exit_code: output.exit_code,
                stdout_tail,
                stderr_tail,
                stdout_truncated,
                stderr_truncated,
            }
        }
        Ok(Err(error)) => {
            let (stderr_tail, stderr_truncated) =
                truncate_tail_bytes(&error.to_string(), VALIDATION_TAIL_BYTES);
            ValidationOutput {
                label: command.label.clone(),
                exit_code: -1,
                stdout_tail: String::new(),
                stderr_tail,
                stdout_truncated: false,
                stderr_truncated,
            }
        }
        Err(_) => ValidationOutput {
            label: command.label.clone(),
            exit_code: -1,
            stdout_tail: String::new(),
            stderr_tail: format!("validation command timed out after {}s", timeout_secs),
            stdout_truncated: false,
            stderr_truncated: false,
        },
    }
}

fn normalize_timeout(timeout_secs: u64) -> u64 {
    if timeout_secs == 0 {
        DEFAULT_TIMEOUT_SECS
    } else {
        timeout_secs
    }
}

/// Returns true only when a `test:` target appears at column zero in the Makefile.
/// Indented lines (e.g. recipe lines that happen to contain `test:`) are not matched.
fn makefile_has_test_target(root: &Path) -> bool {
    let makefile = root.join("Makefile");
    if !makefile.is_file() {
        return false;
    }

    let Ok(content) = fs::read_to_string(makefile) else {
        return false;
    };
    content
        .lines()
        .any(|line| line.starts_with("test:"))
}

fn load_validate_toml(raw: &str) -> std::result::Result<Vec<ValidationCommand>, toml::de::Error> {
    let config = toml::from_str::<ValidateConfig>(raw)?;
    Ok(config
        .commands
        .into_iter()
        .filter_map(|mut command| {
            if command.label.trim().is_empty() || command.program.trim().is_empty() {
                return None;
            }
            command.timeout_secs = normalize_timeout(command.timeout_secs);
            Some(command)
        })
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::{load_validate_toml, makefile_has_test_target, ValidationOutput, ValidationResult, ValidationSuite};
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
        // `test:` appears only as an indented recipe continuation — must not match.
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
        fs::write(
            workspace.path().join("Makefile"),
            "test:\n\tcargo test\n",
        )
        .expect("write Makefile");
        assert!(
            makefile_has_test_target(workspace.path()),
            "column-zero test: must be detected"
        );
    }
}
