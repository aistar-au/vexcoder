use super::*;
use crate::config::UndoConfig;
use std::fs;
use std::path::PathBuf;

fn make_mgr(working_dir: PathBuf) -> ConversationManager {
    let client = ApiClient::new_mock(Arc::new(MockApiClient::new(vec![])));
    let operator = ToolOperator::new(working_dir);
    ConversationManager::new(client, operator)
}

#[test]
fn test_push_and_pop_checkpoint() {
    let mut mgr = make_mgr(std::env::temp_dir());

    assert_eq!(mgr.undo_stack_len(), 0);
    assert!(mgr.pop_undo_checkpoint().is_none());

    let cp = UndoCheckpoint {
        tool_name: "write_file".to_string(),
        path: PathBuf::from("/tmp/a.txt"),
        previous_content: Some("hello".to_string()),
    };
    mgr.push_undo_checkpoint(cp);
    assert_eq!(mgr.undo_stack_len(), 1);

    let popped = mgr.pop_undo_checkpoint().unwrap();
    assert_eq!(popped.tool_name, "write_file");
    assert_eq!(popped.path, PathBuf::from("/tmp/a.txt"));
    assert_eq!(popped.previous_content, Some("hello".to_string()));
    assert_eq!(mgr.undo_stack_len(), 0);
}

#[test]
fn test_stack_lifo_ordering() {
    let mut mgr = make_mgr(std::env::temp_dir());

    for i in 0..3 {
        mgr.push_undo_checkpoint(UndoCheckpoint {
            tool_name: format!("tool_{}", i),
            path: PathBuf::from(format!("/tmp/{}.txt", i)),
            previous_content: None,
        });
    }
    assert_eq!(mgr.undo_stack_len(), 3);

    let last = mgr.pop_undo_checkpoint().unwrap();
    assert_eq!(last.tool_name, "tool_2");

    let mid = mgr.pop_undo_checkpoint().unwrap();
    assert_eq!(mid.tool_name, "tool_1");

    let first = mgr.pop_undo_checkpoint().unwrap();
    assert_eq!(first.tool_name, "tool_0");

    assert!(mgr.pop_undo_checkpoint().is_none());
}

#[test]
fn test_max_checkpoint_eviction() {
    let mut mgr = make_mgr(std::env::temp_dir()).with_max_undo_checkpoints(3);

    for i in 0..5 {
        mgr.push_undo_checkpoint(UndoCheckpoint {
            tool_name: format!("tool_{}", i),
            path: PathBuf::from(format!("/tmp/{}.txt", i)),
            previous_content: None,
        });
    }
    // Only 3 remain (tools 2, 3, 4); oldest were evicted.
    assert_eq!(mgr.undo_stack_len(), 3);

    let cp = mgr.pop_undo_checkpoint().unwrap();
    assert_eq!(cp.tool_name, "tool_4");
    let cp = mgr.pop_undo_checkpoint().unwrap();
    assert_eq!(cp.tool_name, "tool_3");
    let cp = mgr.pop_undo_checkpoint().unwrap();
    assert_eq!(cp.tool_name, "tool_2");
}

#[test]
fn test_zero_max_disables_undo() {
    let mut mgr = make_mgr(std::env::temp_dir()).with_max_undo_checkpoints(0);

    mgr.push_undo_checkpoint(UndoCheckpoint {
        tool_name: "write_file".to_string(),
        path: PathBuf::from("/tmp/a.txt"),
        previous_content: None,
    });
    assert_eq!(mgr.undo_stack_len(), 0);
}

#[test]
fn test_capture_snapshot_existing_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("hello.txt");
    fs::write(&file_path, "original content").unwrap();

    let mgr = make_mgr(dir.path().to_path_buf());

    let input = json!({ "path": "hello.txt" });
    let cp = mgr.capture_undo_snapshot("write_file", &input).unwrap();
    assert_eq!(cp.tool_name, "write_file");
    assert_eq!(cp.previous_content, Some("original content".to_string()));
}

#[test]
fn test_capture_snapshot_nonexistent_file() {
    let dir = TempDir::new().unwrap();
    let mgr = make_mgr(dir.path().to_path_buf());

    let input = json!({ "path": "does_not_exist.txt" });
    let cp = mgr.capture_undo_snapshot("write_file", &input).unwrap();
    assert_eq!(cp.tool_name, "write_file");
    assert!(cp.previous_content.is_none());
}

#[test]
fn test_capture_snapshot_non_mutating_tool_returns_none() {
    let dir = TempDir::new().unwrap();
    let mgr = make_mgr(dir.path().to_path_buf());

    let input = json!({ "path": "hello.txt" });
    assert!(mgr.capture_undo_snapshot("read_file", &input).is_none());
    assert!(mgr.capture_undo_snapshot("list_files", &input).is_none());
    assert!(mgr.capture_undo_snapshot("bash", &input).is_none());
}

#[test]
fn test_capture_snapshot_rename_file() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("old.txt");
    fs::write(&source, "rename me").unwrap();

    let mgr = make_mgr(dir.path().to_path_buf());

    let input = json!({ "old_path": "old.txt", "new_path": "new.txt" });
    let cp = mgr.capture_undo_snapshot("rename_file", &input).unwrap();
    assert_eq!(cp.tool_name, "rename_file");
    assert_eq!(cp.previous_content, Some("rename me".to_string()));
}

#[test]
fn test_resolve_undo_config_defaults() {
    let cfg = UndoConfig::default();
    assert!(cfg.enabled);
    assert_eq!(cfg.max_checkpoints, 20);
}

#[test]
fn test_checkpoint_with_none_content() {
    let cp = UndoCheckpoint {
        tool_name: "write_file".to_string(),
        path: PathBuf::from("/tmp/new.txt"),
        previous_content: None,
    };
    // None means the file did not exist before the mutation
    assert!(cp.previous_content.is_none());
    assert_eq!(cp.tool_name, "write_file");
}
