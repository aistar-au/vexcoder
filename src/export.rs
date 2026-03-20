use crate::runtime::TaskState;
use crate::turn_evidence::{SummaryRecord, TurnEvidenceRecord};
use anyhow::{bail, Result};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Jsonl,
    Markdown,
}

impl ExportFormat {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "jsonl" => Ok(Self::Jsonl),
            "markdown" => Ok(Self::Markdown),
            other => bail!("unsupported export format '{other}'"),
        }
    }
}

pub fn render_task_export(state: &TaskState, format: ExportFormat) -> Result<String> {
    match format {
        ExportFormat::Jsonl => render_jsonl(state),
        ExportFormat::Markdown => Ok(render_markdown(state)),
    }
}

pub fn write_export_output(
    content: &str,
    output_path: Option<&Path>,
    force: bool,
) -> Result<Option<String>> {
    if let Some(path) = output_path {
        if path.exists() && !force {
            bail!(
                "export output '{}' already exists; pass --force to overwrite",
                path.display()
            );
        }
        std::fs::write(path, content)?;
        Ok(Some(format!("{}", path.display())))
    } else {
        Ok(None)
    }
}

fn render_jsonl(state: &TaskState) -> Result<String> {
    let mut lines = state
        .turns
        .iter()
        .enumerate()
        .map(|(index, turn)| {
            serde_json::to_string(&TurnEvidenceRecord {
                turn: index + 1,
                input: turn.input.clone(),
                response: turn.response.clone(),
                instructions_path: state.instructions_path.clone(),
                changed_files: turn.changed_files.clone(),
                command_history: turn.command_history.clone(),
                tokens: turn.tokens,
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;

    lines.push(serde_json::to_string(&SummaryRecord {
        summary: true,
        status: format!("{:?}", state.status),
        task_id: state.id.clone(),
        total_turns: state.turns.len(),
        instructions_path: state.instructions_path.clone(),
        changed_files: state
            .changed_files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
    })?);

    Ok(lines.join("\n"))
}

fn render_markdown(state: &TaskState) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Task export: `{}`\n\n", state.id));
    out.push_str(&format!("- Status: `{:?}`\n", state.status));
    out.push_str(&format!("- Turns: `{}`\n", state.turns.len()));
    out.push_str(&format!(
        "- Instructions: `{}`\n\n",
        state.instructions_path.as_deref().unwrap_or("none")
    ));

    out.push_str("## Changed files\n\n");
    if state.changed_files.is_empty() {
        out.push_str("_None_\n\n");
    } else {
        out.push_str("| Path |\n| --- |\n");
        for path in &state.changed_files {
            out.push_str(&format!("| `{}` |\n", path.to_string_lossy()));
        }
        out.push('\n');
    }

    out.push_str("## Command history\n\n");
    if state.command_history.is_empty() {
        out.push_str("_None_\n\n");
    } else {
        for command in &state.command_history {
            let exit = command
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "interrupted".to_string());
            out.push_str(&format!("- `{}` — exit `{exit}`\n", command.program));
        }
        out.push('\n');
    }

    out.push_str("## Turns\n\n");
    if state.turns.is_empty() {
        out.push_str("_No turn evidence persisted._\n");
        return out;
    }

    for (index, turn) in state.turns.iter().enumerate() {
        out.push_str(&format!("### Turn {}\n\n", index + 1));
        if turn.tool_invocations.is_empty() {
            out.push_str("- Tools: none recorded\n\n");
            continue;
        }
        for tool in &turn.tool_invocations {
            out.push_str(&format!("- `{}` — `{}`\n", tool.name, tool.outcome));
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{render_task_export, write_export_output, ExportFormat};
    use crate::runtime::{TaskState, TaskStatus};
    use crate::turn_evidence::{ToolInvocationSummary, TurnEvidenceState};
    use crate::usage::TurnTokens;

    #[test]
    fn test_vex_export_jsonl_matches_batch_schema() {
        let mut state = TaskState::new("task-export".to_string());
        state.status = TaskStatus::Completed;
        state.instructions_path = Some("AGENTS.md".to_string());
        state.turns.push(TurnEvidenceState {
            input: "hello".to_string(),
            response: "world".to_string(),
            changed_files: vec!["src/lib.rs".to_string()],
            command_history: Vec::new(),
            tool_invocations: vec![ToolInvocationSummary {
                step_id: 1,
                name: "read_file".to_string(),
                outcome: "ok".to_string(),
                duration_ms: Some(12),
            }],
            tokens: TurnTokens {
                input: 5,
                output: 7,
                estimated: false,
            },
        });

        let rendered = render_task_export(&state, ExportFormat::Jsonl).expect("jsonl");
        let lines = rendered.lines().collect::<Vec<_>>();
        let turn: serde_json::Value = serde_json::from_str(lines[0]).expect("turn json");
        assert_eq!(turn["turn"], 1);
        assert!(turn["tokens"].is_object());
        assert_eq!(turn["tokens"]["input"], 5);
        assert!(turn.get("response").is_some());
    }

    #[test]
    fn test_vex_export_markdown_omits_model_response_text() {
        let mut state = TaskState::new("task-export".to_string());
        state.turns.push(TurnEvidenceState {
            input: "hello".to_string(),
            response: "sensitive full response".to_string(),
            changed_files: Vec::new(),
            command_history: Vec::new(),
            tool_invocations: vec![ToolInvocationSummary {
                step_id: 1,
                name: "apply_patch".to_string(),
                outcome: "ok".to_string(),
                duration_ms: Some(34),
            }],
            tokens: TurnTokens::default(),
        });

        let rendered = render_task_export(&state, ExportFormat::Markdown).expect("markdown");
        assert!(!rendered.contains("sensitive full response"));
        assert!(rendered.contains("`apply_patch`"));
    }

    #[test]
    fn test_vex_export_force_flag_overwrites() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("export.md");
        std::fs::write(&path, "old").expect("seed");

        let first = write_export_output("new", Some(&path), false);
        assert!(first.is_err(), "must refuse overwrite without force");

        write_export_output("new", Some(&path), true).expect("force write");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "new");
    }
}
