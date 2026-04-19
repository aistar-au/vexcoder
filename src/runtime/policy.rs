pub trait RuntimeCorePolicy {
    fn request_requires_tool_evidence(&self, input: &str) -> bool;
    fn tool_retry_instruction(&self) -> &'static str;
    fn repeated_tool_round_instruction(&self) -> &'static str;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultRuntimeCorePolicy;

const TOOL_RETRY_INSTRUCTION: &str = "Your previous answer did not execute any tool call. This request \
requires tool-backed evidence from the workspace. Call the appropriate tool now before \
answering.";

const REPEATED_TOOL_ROUND_INSTRUCTION: &str = "You repeated the same read/search tool call with unchanged arguments. \
Do not repeat identical tool calls. Use existing tool results to answer now. \
Only call a different tool if new evidence is required.";

const TOOL_REQUIRED_HINTS: [&str; 29] = [
    "file",
    "files",
    "directory",
    "directories",
    "tree",
    "repo",
    "repository",
    "cargo.toml",
    "readme",
    "docs/",
    "src/",
    "tests/",
    "version",
    "versions",
    "pinned",
    "count",
    "how many",
    "list",
    "show",
    "search",
    "find",
    "path",
    "line",
    "content of",
    "what's in",
    "whats in",
    "what is in",
    "read it again",
    "read again",
];

pub fn default_runtime_policy() -> DefaultRuntimeCorePolicy {
    DefaultRuntimeCorePolicy
}

impl RuntimeCorePolicy for DefaultRuntimeCorePolicy {
    fn request_requires_tool_evidence(&self, input: &str) -> bool {
        let normalized = input.to_ascii_lowercase();
        TOOL_REQUIRED_HINTS
            .iter()
            .any(|hint| normalized.contains(hint))
    }

    fn tool_retry_instruction(&self) -> &'static str {
        TOOL_RETRY_INSTRUCTION
    }

    fn repeated_tool_round_instruction(&self) -> &'static str {
        REPEATED_TOOL_ROUND_INSTRUCTION
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeCorePolicy, default_runtime_policy};

    #[test]
    fn test_request_requires_tool_evidence_detects_repo_facts() {
        let policy = default_runtime_policy();
        assert!(policy.request_requires_tool_evidence("how many files are in this tree"));
        assert!(policy.request_requires_tool_evidence("what's in docs/src/"));
        assert!(!policy.request_requires_tool_evidence("say hello"));
    }
}
