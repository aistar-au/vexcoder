use super::{
    ApprovalScope, Capability, CommandRequest, DefaultCommandRunner, GenerateTestsArgs,
    ResumeTaskEntry, ReviewArgs, TaskState, ValidationSuite, DEFAULT_MAX_HISTORY_LINES,
    MAX_HISTORY_LINES_ENV, SLASH_COMMANDS,
};
use anyhow::Result;
use std::path::PathBuf;

pub(super) fn builtin_slash_command_names() -> Vec<String> {
    let mut names = SLASH_COMMANDS
        .iter()
        .filter_map(|spec| {
            spec.display
                .strip_prefix('/')
                .and_then(|display| display.split_whitespace().next())
                .map(ToString::to_string)
        })
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

pub(super) fn parse_generate_tests_args(
    input: &str,
) -> std::result::Result<GenerateTestsArgs, String> {
    let mut parsed = GenerateTestsArgs::default();
    let mut tokens = input.split_whitespace();

    while let Some(token) = tokens.next() {
        if token == "--framework" {
            let Some(framework) = tokens.next() else {
                return Err(
                    "[generate-tests] usage: /generate-tests [path] [--framework <name>]"
                        .to_string(),
                );
            };
            parsed.framework = Some(framework.to_string());
            continue;
        }

        if parsed.path.is_some() {
            return Err(
                "[generate-tests] usage: /generate-tests [path] [--framework <name>]".to_string(),
            );
        }
        parsed.path = Some(token.to_string());
    }

    Ok(parsed)
}

pub(super) fn parse_review_args(input: &str) -> std::result::Result<ReviewArgs, String> {
    const USAGE: &str =
        "[review] usage: /review [--base <git-ref>] [--files <glob>] [<instruction>]";

    let mut parsed = ReviewArgs::default();
    let mut tokens = input.split_whitespace();
    let mut instruction_tokens = Vec::new();

    while let Some(token) = tokens.next() {
        match token {
            "--base" if instruction_tokens.is_empty() => {
                let Some(base_ref) = tokens.next() else {
                    return Err(USAGE.to_string());
                };
                if parsed.base.replace(base_ref.to_string()).is_some() {
                    return Err(USAGE.to_string());
                }
            }
            "--files" if instruction_tokens.is_empty() => {
                let Some(files_glob) = tokens.next() else {
                    return Err(USAGE.to_string());
                };
                if parsed.files.replace(files_glob.to_string()).is_some() {
                    return Err(USAGE.to_string());
                }
            }
            _ => {
                instruction_tokens.push(token.to_string());
                instruction_tokens.extend(tokens.map(ToString::to_string));
                break;
            }
        }
    }

    if parsed.base.is_some() && parsed.files.is_some() {
        return Err("[review: --base and --files are mutually exclusive]".to_string());
    }

    if !instruction_tokens.is_empty() {
        parsed.instruction = Some(instruction_tokens.join(" "));
    }

    Ok(parsed)
}

pub fn capability_to_kebab(cap: Capability) -> &'static str {
    match cap {
        Capability::ReadFile => "read-file",
        Capability::WriteFile => "write-file",
        Capability::ApplyPatch => "apply-patch",
        Capability::RunCommand => "run-command",
        Capability::Network => "network",
        Capability::Browser => "browser",
    }
}

pub fn kebab_to_capability(s: &str) -> Option<Capability> {
    match s {
        "read-file" => Some(Capability::ReadFile),
        "write-file" => Some(Capability::WriteFile),
        "apply-patch" => Some(Capability::ApplyPatch),
        "run-command" => Some(Capability::RunCommand),
        "network" => Some(Capability::Network),
        "browser" => Some(Capability::Browser),
        _ => None,
    }
}

pub(super) fn capability_for_tool_name(tool_name: &str) -> Option<Capability> {
    match tool_name {
        "read_file" | "list_files" | "list_directory" | "search" | "search_files"
        | "search_content" | "find_files" | "git_status" | "git_diff" | "git_log" | "git_show" => {
            Some(Capability::ReadFile)
        }
        "write_file" | "edit_file" | "rename_file" => Some(Capability::WriteFile),
        "apply_patch" | "git_add" | "git_commit" => Some(Capability::ApplyPatch),
        "run_command" => Some(Capability::RunCommand),
        _ => None,
    }
}

pub(super) fn scope_to_label(scope: ApprovalScope) -> &'static str {
    match scope {
        ApprovalScope::Once => "once",
        ApprovalScope::Task => "task",
        ApprovalScope::Session => "session",
    }
}

pub(super) fn kebab_to_scope(s: &str) -> Option<ApprovalScope> {
    match s {
        "once" => Some(ApprovalScope::Once),
        "session" => Some(ApprovalScope::Session),
        _ => None,
    }
}

pub(super) fn shell_command_request(command: String, working_dir: PathBuf) -> CommandRequest {
    if cfg!(windows) {
        CommandRequest {
            program: "cmd".to_string(),
            args: vec!["/C".to_string(), command],
            working_dir: Some(working_dir),
        }
    } else {
        CommandRequest {
            program: "sh".to_string(),
            args: vec!["-c".to_string(), command],
            working_dir: Some(working_dir),
        }
    }
}

pub(super) fn format_inline_block(
    kind: &str,
    path: &str,
    content: &str,
    truncated: bool,
    byte_limit: Option<usize>,
) -> String {
    let mut rendered = format!("[{kind}: {path}]\n```text\n{content}\n```");
    if truncated {
        if let Some(limit) = byte_limit {
            rendered.push_str(&format!(
                "\n[{kind}: {path} \u{2014} truncated to {limit} bytes]"
            ));
        } else {
            rendered.push_str(&format!("\n[{kind}: {path} \u{2014} truncated]"));
        }
    }
    rendered
}

pub(super) async fn run_validation_suite_capture(
    suite: ValidationSuite,
    working_dir: PathBuf,
) -> Result<crate::runtime::ValidationResult> {
    // Validation commands are operator-defined build/test steps from the
    // project config.  They run under the same user session that launched vex,
    // so no sandbox wrapping is applied.  A future ADR-024 follow-up may
    // thread the operator-configured sandbox driver into validation execution.
    let runner = DefaultCommandRunner::new();
    suite.run_in_dir(&runner, Some(&working_dir)).await
}

pub(super) fn new_task_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static LAST_TASK_MS: AtomicU64 = AtomicU64::new(0);

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let ms = LAST_TASK_MS.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |previous| {
        Some(now_ms.max(previous.saturating_add(1)))
    });
    // `fetch_update` returns the previous value, so recompute the stored
    // monotonic millisecond from that prior state before formatting the id.
    let stable_ms = ms
        .map(|previous| now_ms.max(previous.saturating_add(1)))
        .unwrap_or(now_ms);

    format!("task-{stable_ms}")
}

pub(super) fn resolve_history_line_cap() -> usize {
    std::env::var(MAX_HISTORY_LINES_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|cap| *cap > 0)
        .unwrap_or(DEFAULT_MAX_HISTORY_LINES)
}

pub(super) fn resolve_repo_label() -> String {
    std::env::var("VEX_REPO_LABEL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or_else(|| "workspace".to_string())
}

pub(super) fn sanitize_task_label(label: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;

    for ch in label.trim().chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if matches!(ch, '-' | '_' | ' ') {
            Some('-')
        } else {
            None
        };

        let Some(ch) = normalized else {
            continue;
        };
        if ch == '-' {
            if out.is_empty() || last_was_dash {
                continue;
            }
            last_was_dash = true;
        } else {
            last_was_dash = false;
        }
        out.push(ch);
    }

    out.trim_matches('-').to_string()
}

pub(super) fn summarize_tool_outcome(output: &str, is_error: bool) -> &'static str {
    if !is_error {
        return "ok";
    }

    let lowered = output.to_ascii_lowercase();
    if lowered.contains("denied") {
        "denied"
    } else if lowered.contains("cancel") {
        "cancelled"
    } else {
        "error"
    }
}

pub(super) fn list_recent_task_entries(limit: usize) -> Vec<ResumeTaskEntry> {
    TaskState::state_files()
        .into_iter()
        .take(limit)
        .map(|file| match TaskState::load(&file.dir, &file.id) {
            Ok(state) => ResumeTaskEntry {
                id: state.id,
                status: format!("{:?}", state.status),
            },
            Err(_) => ResumeTaskEntry {
                id: file.id,
                status: "Unreadable".to_string(),
            },
        })
        .collect()
}
