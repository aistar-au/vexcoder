use super::*;

pub(crate) fn is_read_only_user_request(input: &str) -> bool {
    if std::env::var("VEX_FORCE_MUTATING_TURN").as_deref() == Ok("1") {
        return false;
    }
    const READ_ONLY_HINTS: [&str; 25] = [
        "show",
        "read",
        "list",
        "count",
        "how many",
        "read-only",
        "read only",
        "readonly",
        "review only",
        "no changes",
        "without changes",
        "do not change",
        "don't change",
        "do not modify",
        "don't modify",
        "what is in",
        "what's in",
        "whats in",
        "content of",
        "status",
        "diff",
        "log",
        "cat",
        "display",
        "print",
    ];
    const MUTATING_HINTS: [&str; 18] = [
        "write",
        "edit",
        "update",
        "create",
        "add",
        "delete",
        "remove",
        "rename",
        "move",
        "commit",
        "stage",
        "patch",
        "apply",
        "implement",
        "refactor",
        "fix",
        "push",
        "rebase",
    ];

    let normalized = input.to_ascii_lowercase();
    let has_read_only_hint = READ_ONLY_HINTS
        .iter()
        .any(|hint| contains_hint(&normalized, hint));
    let has_mutating_hint = MUTATING_HINTS
        .iter()
        .any(|hint| contains_hint(&normalized, hint));

    has_read_only_hint && !has_mutating_hint
}

fn contains_hint(normalized: &str, hint: &str) -> bool {
    normalized.match_indices(hint).any(|(start, _)| {
        let end = start + hint.len();
        let previous = normalized[..start].chars().next_back();
        let next = normalized[end..].chars().next();

        previous.is_none_or(|value| !value.is_ascii_alphanumeric())
            && next.is_none_or(|value| !value.is_ascii_alphanumeric())
    })
}

pub(crate) fn mutating_tool_read_only_conflict_prompt(
    user_input: &str,
    tool_name: &str,
) -> Option<String> {
    if !tool_requires_confirmation(tool_name) || !is_read_only_user_request(user_input) {
        return None;
    }

    Some(format!(
        "Blocked mutating tool call `{tool_name}` because this request appears read-only. Use read-only tools (`read_file`, `search_files`, `list_files`, `list_dir`, `glob_files`, `git_status`, `git_diff`, `git_log`, `git_show`) and answer from those results. No file changes were made."
    ))
}

pub(crate) fn tests_only_mutation_conflict_prompt(
    policy: PulseToolPolicy,
    tool_name: &str,
    input: &serde_json::Value,
) -> Option<String> {
    if policy != PulseToolPolicy::TestsOnlyMutations {
        return None;
    }

    let target_paths = mutating_tool_target_paths(tool_name, input);
    if target_paths.is_empty() || target_paths.iter().all(|path| is_test_target_path(path)) {
        return None;
    }

    let blocked_path = target_paths
        .into_iter()
        .find(|path| !is_test_target_path(path))
        .unwrap_or_default();
    Some(format!(
        "Dropped non-test patch target `{blocked_path}` for /generate-tests. This command may only modify test files or paths under `test/` or `tests/`. Use `/edit` for source-file changes."
    ))
}

fn mutating_tool_target_paths(tool_name: &str, input: &serde_json::Value) -> Vec<String> {
    match tool_name {
        "write_file" | "edit_file" => {
            first_tool_string(input, &["path", "file_path", "file", "filename"])
                .into_iter()
                .map(ToString::to_string)
                .collect()
        }
        "apply_patch" => first_tool_string(input, &["path", "file_path", "file", "filename"])
            .map(ToString::to_string)
            .into_iter()
            .chain(
                first_tool_string(input, &["content", "patch", "diff"])
                    .and_then(extract_apply_patch_target_path),
            )
            .collect(),
        "rename_file" => [
            first_tool_string(input, &["old_path", "from", "source_path"]),
            first_tool_string(input, &["new_path", "to", "target_path"]),
        ]
        .into_iter()
        .flatten()
        .map(ToString::to_string)
        .collect(),
        _ => Vec::new(),
    }
}

fn extract_apply_patch_target_path(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let path = line
            .strip_prefix("+++ b/")
            .or_else(|| line.strip_prefix("--- a/"))
            .or_else(|| {
                line.strip_prefix("diff --git a/")
                    .and_then(|rest| rest.split_once(" b/").map(|(_, path)| path))
            })?;
        let normalized = path.trim();
        if normalized.is_empty() || normalized == "/dev/null" {
            None
        } else {
            Some(normalized.to_string())
        }
    })
}

pub(super) fn is_test_target_path(path: &str) -> bool {
    let normalized = path.trim().replace('\\', "/").to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }

    if normalized
        .split('/')
        .any(|segment| segment == "test" || segment == "tests" || segment == "__tests__")
    {
        return true;
    }

    let file_name = normalized.rsplit('/').next().unwrap_or(normalized.as_str());
    matches!(file_name, "test.rs" | "tests.rs")
        || file_name.starts_with("test_")
        || file_name.starts_with("spec_")
        || file_name.contains(".test.")
        || file_name.contains(".spec.")
        || file_name.ends_with("_test.rs")
        || file_name.ends_with("_tests.rs")
        || file_name.ends_with("_test.py")
        || file_name.ends_with("_test.go")
        || file_name.ends_with("_test.js")
        || file_name.ends_with("_test.jsx")
        || file_name.ends_with("_test.ts")
        || file_name.ends_with("_test.tsx")
        || file_name.ends_with("_spec.rb")
}
