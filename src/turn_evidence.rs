use crate::runtime::CommandEvidence;
use crate::usage::TurnTokens;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnEvidenceRecord {
    pub turn: usize,
    pub input: String,
    pub response: String,
    pub instructions_path: Option<String>,
    pub changed_files: Vec<String>,
    pub command_history: Vec<CommandEvidence>,
    pub tokens: TurnTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SummaryRecord {
    pub summary: bool,
    pub status: String,
    pub task_id: String,
    pub total_turns: usize,
    pub instructions_path: Option<String>,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolInvocationSummary {
    /// Stable identity carried from [`PendingTurnToolCall`] on completion.
    #[serde(default)]
    pub step_id: u64,
    pub name: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnEvidenceState {
    pub input: String,
    pub response: String,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub command_history: Vec<CommandEvidence>,
    #[serde(default)]
    pub tool_invocations: Vec<ToolInvocationSummary>,
    #[serde(default)]
    pub tokens: TurnTokens,
}

pub fn first_string_field<'a>(input: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub fn note_changed_files_from_tool_call(
    changed_files: &mut BTreeSet<String>,
    name: &str,
    input: &serde_json::Value,
) {
    match name {
        "write_file" | "apply_patch" | "edit_file" | "git_add" => {
            if let Some(path) =
                first_string_field(input, &["path", "file_path", "file", "filename"])
            {
                changed_files.insert(path.to_string());
            }
        }
        "rename_file" => {
            for key in [
                "old_path",
                "from",
                "source_path",
                "new_path",
                "to",
                "target_path",
            ] {
                if let Some(path) = first_string_field(input, &[key]) {
                    changed_files.insert(path.to_string());
                }
            }
        }
        _ => {}
    }
}

pub fn command_evidence_from_tool_result(name: &str, is_error: bool) -> Option<CommandEvidence> {
    let program = match name {
        "git_status" => Some("git status"),
        "git_diff" => Some("git diff"),
        "git_log" => Some("git log"),
        "git_show" => Some("git show"),
        "git_add" => Some("git add"),
        "git_commit" => Some("git commit"),
        _ => None,
    }?;

    Some(CommandEvidence {
        program: program.to_string(),
        exit_code: Some(if is_error { 1 } else { 0 }),
        interrupted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::{command_evidence_from_tool_result, note_changed_files_from_tool_call};
    use std::collections::BTreeSet;

    #[test]
    fn note_changed_files_collects_primary_paths() {
        let mut changed = BTreeSet::new();
        note_changed_files_from_tool_call(
            &mut changed,
            "write_file",
            &serde_json::json!({"path":"src/main.rs"}),
        );
        assert!(changed.contains("src/main.rs"));
    }

    #[test]
    fn note_changed_files_collects_rename_paths() {
        let mut changed = BTreeSet::new();
        note_changed_files_from_tool_call(
            &mut changed,
            "rename_file",
            &serde_json::json!({"old_path":"a.txt","new_path":"b.txt"}),
        );
        assert!(changed.contains("a.txt"));
        assert!(changed.contains("b.txt"));
    }

    #[test]
    fn command_evidence_maps_git_tools() {
        let evidence =
            command_evidence_from_tool_result("git_commit", false).expect("git evidence");
        assert_eq!(evidence.program, "git commit");
        assert_eq!(evidence.exit_code, Some(0));
    }
}
