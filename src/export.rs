use crate::pulse_evidence::{SummaryRecord, TurnEvidenceRecord};
use crate::runtime::TaskState;
use anyhow::{Result, bail};
use base64::Engine as _;
use std::path::Path;

pub fn encode_base64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

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
        .pulses
        .iter()
        .enumerate()
        .map(|(index, pulse)| {
            serde_json::to_string(&TurnEvidenceRecord {
                pulse: index + 1,
                input: pulse.input.clone(),
                response: pulse.response.clone(),
                instructions_path: state.instructions_path.clone(),
                changed_files: pulse.changed_files.clone(),
                command_history: pulse.command_history.clone(),
                tokens: pulse.tokens,
            })
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;

    lines.push(serde_json::to_string(&SummaryRecord {
        summary: true,
        status: format!("{:?}", state.status),
        task_id: state.id.clone(),
        total_turns: state.pulses.len(),
        instructions_path: state.instructions_path.clone(),
        changed_files: state
            .changed_files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        session_tasks: state
            .session_tasks
            .iter()
            .map(|task| format!("{}:{:?}", task.id, task.lifecycle_state))
            .collect(),
    })?);

    Ok(lines.join("\n"))
}

fn render_markdown(state: &TaskState) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Task export: `{}`\n\n", state.id));
    out.push_str(&format!("- Status: `{:?}`\n", state.status));
    out.push_str(&format!("- Pulses: `{}`\n", state.pulses.len()));
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

    out.push_str("## Session tasks\n\n");
    if state.session_tasks.is_empty() {
        out.push_str("_None_\n\n");
    } else {
        for task in &state.session_tasks {
            out.push_str(&format!(
                "- `{}` — agent `{}` status `{:?}`\n",
                task.id, task.agent_id, task.lifecycle_state
            ));
        }
        out.push('\n');
    }

    out.push_str("## Pulses\n\n");
    if state.pulses.is_empty() {
        out.push_str("_No pulse evidence persisted._\n");
        return out;
    }

    for (index, pulse) in state.pulses.iter().enumerate() {
        out.push_str(&format!("### Pulse {}\n\n", index + 1));
        if pulse.tool_invocations.is_empty() {
            out.push_str("- Tools: none recorded\n\n");
            continue;
        }
        for tool in &pulse.tool_invocations {
            out.push_str(&format!("- `{}` — `{}`\n", tool.name, tool.outcome));
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{ExportFormat, render_task_export, write_export_output};
    use crate::pulse_evidence::{ToolInvocationSummary, TurnEvidenceState};
    use crate::runtime::{TaskState, TaskStatus};
    use crate::usage::PulseTokens;

    #[test]
    fn test_vex_export_jsonl_matches_batch_schema() {
        let mut state = TaskState::new("task-export".to_string());
        state.status = TaskStatus::Completed;
        state.instructions_path = Some("AGENTS.md".to_string());
        state.pulses.push(TurnEvidenceState {
            input: "explain the build error".to_string(),
            response: "world".to_string(),
            changed_files: vec!["src/lib.rs".to_string()],
            command_history: Vec::new(),
            tool_invocations: vec![ToolInvocationSummary {
                step_id: 1,
                name: "read_file".to_string(),
                outcome: "ok".to_string(),
            }],
            tokens: PulseTokens {
                input: 5,
                output: 7,
                estimated: false,
                ..Default::default()
            },
        });

        let rendered = render_task_export(&state, ExportFormat::Jsonl).expect("jsonl");
        let lines = rendered.lines().collect::<Vec<_>>();
        let pulse: serde_json::Value = serde_json::from_str(lines[0]).expect("pulse json");
        assert_eq!(pulse["pulse"], 1);
        assert!(pulse["tokens"].is_object());
        assert_eq!(pulse["tokens"]["input"], 5);
        assert!(pulse.get("response").is_some());
    }

    #[test]
    fn test_vex_export_markdown_omits_model_response_text() {
        let mut state = TaskState::new("task-export".to_string());
        state.pulses.push(TurnEvidenceState {
            input: "explain the build error".to_string(),
            response: "sensitive full response".to_string(),
            changed_files: Vec::new(),
            command_history: Vec::new(),
            tool_invocations: vec![ToolInvocationSummary {
                step_id: 1,
                name: "apply_patch".to_string(),
                outcome: "ok".to_string(),
            }],
            tokens: PulseTokens::default(),
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
