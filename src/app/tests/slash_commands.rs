use super::*;
use crate::mcp::{McpRegistryRollup, McpServerRollup, McpToolSummary};

#[test]
fn non_slash_input_routes_model_pulse() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("describe the open issues".to_string(), &mut ctx);
    assert!(mode.is_pulse_in_progress());
}

#[test]
fn edit_command_passes_bare_instruction_to_pulse_and_blocks_reentry() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/edit refactor the parser".to_string(), &mut ctx);
    assert_eq!(mode.last_turn_input.as_deref(), Some("refactor the parser"));

    let mut mode2 = TuiMode::new();
    let mut ctx2 = setup_ctx();
    mode2.active_edit_loop = Some(EditLoop::new("task-existing".to_string()));
    mode2.begin_turn_capture("placeholder".to_string());
    mode2.on_user_input("/edit add more tests".to_string(), &mut ctx2);
    assert!(
        mode2
            .history_lines()
            .iter()
            .any(|l| l.contains("[edit loop already active"))
    );
}

#[test]
fn run_command_does_not_start_model_pulse() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input(successful_run_input(), &mut ctx);
    assert!(!mode.is_pulse_in_progress());
    assert!(mode.active_edit_loop.is_none());
    assert!(mode.history_lines().iter().any(|l| l.contains("[run]")));
}

#[test]
fn tui_commands_renders_all_registered_commands() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/commands".to_string(), &mut ctx);
    let lines = mode.history_lines();
    for cmd in ["/edit", "/run", "/memory", "/init", "/context"] {
        assert!(
            lines.iter().any(|l| l.contains(cmd)),
            "missing {cmd} from /commands output"
        );
    }
}

#[tokio::test]
async fn context_command_renders_without_model_turn() {
    let mut mode = TuiMode::new();
    let mut ctx = setup_ctx();
    mode.on_user_input("/context".to_string(), &mut ctx);
    assert!(!mode.is_pulse_in_progress());
    let lines = mode.history_lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("~") || l.contains("tokens") || l.contains("context")),
        "context output must include token info; lines: {lines:?}"
    );
}
