pub trait RuntimeCorePolicy {
    fn sanitize_assistant_text(&self, text: &str) -> String;
    fn request_requires_tool_evidence(&self, input: &str) -> bool;
    fn tool_retry_instruction(&self) -> &'static str;
    fn repeated_tool_round_instruction(&self) -> &'static str;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultRuntimeCorePolicy;

const TOOL_RETRY_INSTRUCTION: &str =
    "Your previous answer did not execute any tool call. This request \
requires tool-backed evidence from the workspace. Call the appropriate tool now before \
answering. If structured tool calls are unavailable, use tagged syntax:
<function=tool_name>
<parameter=arg>value</parameter>
</function>";

const REPEATED_TOOL_ROUND_INSTRUCTION: &str =
    "You repeated the same read/search tool call with unchanged arguments. \
Do not repeat identical tool calls. Use existing tool results to answer now. \
Only call a different tool if new evidence is required.";

const TOOL_REQUIRED_TARGET_HINTS: [&str; 18] = [
    "file",
    "files",
    "directory",
    "directories",
    "tree",
    "repo",
    "repository",
    "path",
    "line",
    "workflow",
    "workflows",
    "diff",
    "status",
    "log",
    "commit",
    "commits",
    "branch",
    "branches",
];

const TOOL_REQUIRED_INSPECTION_HINTS: [&str; 18] = [
    "count",
    "how many",
    "list",
    "show",
    "display",
    "print",
    "search",
    "find",
    "review",
    "verify",
    "inspect",
    "check",
    "content of",
    "what's in",
    "whats in",
    "what is in",
    "read it again",
    "read again",
];

const TOOL_REQUIRED_PATH_HINTS: [&str; 9] = [
    ".github/",
    "adr/",
    "docs/",
    "src/",
    "tests/",
    "./",
    "../",
    "cargo.toml",
    "makefile",
];

const TOOL_REQUIRED_FILE_EXTENSIONS: [&str; 12] = [
    ".rs", ".toml", ".md", ".txt", ".yml", ".yaml", ".json", ".jsonl", ".sh", ".py", ".ts", ".js",
];

pub fn default_runtime_policy() -> DefaultRuntimeCorePolicy {
    DefaultRuntimeCorePolicy
}

pub fn sanitize_assistant_text(text: &str) -> String {
    default_runtime_policy().sanitize_assistant_text(text)
}

impl RuntimeCorePolicy for DefaultRuntimeCorePolicy {
    fn sanitize_assistant_text(&self, text: &str) -> String {
        strip_tagged_tool_markup(text)
    }

    fn request_requires_tool_evidence(&self, input: &str) -> bool {
        let normalized = input.to_ascii_lowercase();
        let has_explicit_workspace_target = contains_explicit_workspace_target(&normalized);
        let has_workspace_target = TOOL_REQUIRED_TARGET_HINTS
            .iter()
            .any(|hint| contains_hint(&normalized, hint));
        let has_inspection_intent = TOOL_REQUIRED_INSPECTION_HINTS
            .iter()
            .any(|hint| contains_hint(&normalized, hint));

        has_explicit_workspace_target || (has_workspace_target && has_inspection_intent)
    }

    fn tool_retry_instruction(&self) -> &'static str {
        TOOL_RETRY_INSTRUCTION
    }

    fn repeated_tool_round_instruction(&self) -> &'static str {
        REPEATED_TOOL_ROUND_INSTRUCTION
    }
}

fn contains_explicit_workspace_target(normalized: &str) -> bool {
    TOOL_REQUIRED_PATH_HINTS
        .iter()
        .any(|hint| normalized.contains(hint))
        || normalized
            .split_whitespace()
            .any(token_has_workspace_file_extension)
}

fn token_has_workspace_file_extension(token: &str) -> bool {
    let trimmed = token.trim_matches(|ch: char| {
        !(ch.is_ascii_alphanumeric() || matches!(ch, '/' | '\\' | '.' | '_' | '-'))
    });
    TOOL_REQUIRED_FILE_EXTENSIONS
        .iter()
        .any(|ext| trimmed.ends_with(ext))
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

fn strip_tagged_tool_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;

    while let Some(rel_start) = text[cursor..].find("<function=") {
        let start = cursor + rel_start;
        out.push_str(&text[cursor..start]);

        let Some(rel_end) = text[start..].find("</function>") else {
            return strip_incomplete_tool_tag_suffix(&out);
        };
        cursor = start + rel_end + "</function>".len();
    }

    out.push_str(&text[cursor..]);
    let out = out.replace("<tool_call>", "").replace("</tool_call>", "");
    collapse_blank_runs(&strip_incomplete_tool_tag_suffix(&out))
}

fn strip_incomplete_tool_tag_suffix(text: &str) -> String {
    let mut out = text.to_string();
    let Some(last_open) = out.rfind('<') else {
        return out;
    };

    let suffix = &out[last_open..];
    let suffix_lower = suffix.to_ascii_lowercase();
    let looks_like_incomplete_tool_tag = "<function=".starts_with(&suffix_lower)
        || "<function".starts_with(&suffix_lower)
        || "</function>".starts_with(&suffix_lower)
        || "</function".starts_with(&suffix_lower)
        || "<parameter=".starts_with(&suffix_lower)
        || "<parameter".starts_with(&suffix_lower)
        || "</parameter>".starts_with(&suffix_lower)
        || "</parameter".starts_with(&suffix_lower)
        || "<tool_call>".starts_with(&suffix_lower)
        || "<tool_call".starts_with(&suffix_lower)
        || "</tool_call>".starts_with(&suffix_lower)
        || "</tool_call".starts_with(&suffix_lower);

    if looks_like_incomplete_tool_tag {
        out.truncate(last_open);
    }

    out
}

fn collapse_blank_runs(text: &str) -> String {
    let mut collapsed = text.to_string();
    while collapsed.contains("\n\n\n") {
        collapsed = collapsed.replace("\n\n\n", "\n\n");
    }
    collapsed
}

#[cfg(test)]
mod tests {
    use super::{default_runtime_policy, sanitize_assistant_text, RuntimeCorePolicy};

    #[test]
    fn test_sanitize_assistant_text_removes_tool_block() {
        let text = "Checking.\n<function=git_status>\n</function>\nDone.";
        assert_eq!(sanitize_assistant_text(text), "Checking.\n\nDone.");
    }

    #[test]
    fn test_sanitize_assistant_text_removes_tool_call_wrapper() {
        let text = "Checking.\n<tool_call>\n<function=git_status>\n</function>\nDone.";
        assert_eq!(sanitize_assistant_text(text), "Checking.\n\nDone.");
    }

    #[test]
    fn test_sanitize_assistant_text_drops_incomplete_tag_suffix() {
        let text = "Checking.\n<function=git_status";
        assert_eq!(sanitize_assistant_text(text), "Checking.\n");
    }

    #[test]
    fn test_sanitize_assistant_text_drops_incomplete_tool_call_wrapper_suffix() {
        let text = "Checking.\n<tool_call";
        assert_eq!(sanitize_assistant_text(text), "Checking.\n");
    }

    #[test]
    fn test_request_requires_tool_evidence_detects_repo_facts() {
        let policy = default_runtime_policy();
        assert!(policy.request_requires_tool_evidence("how many files are in this tree"));
        assert!(policy.request_requires_tool_evidence("what's in docs/src/"));
        assert!(policy.request_requires_tool_evidence("review .github/workflows/nightly.yml"));
        assert!(policy.request_requires_tool_evidence("print the git diff"));
        assert!(!policy.request_requires_tool_evidence("print a tiny async tokio calculator"));
        assert!(!policy.request_requires_tool_evidence("say hello"));
    }
}
