use super::*;
use std::path::PathBuf;

fn make_mgr(working_dir: PathBuf) -> ConversationManager {
    ConversationManager::new(
        ApiClient::new_mock(Arc::new(MockApiClient::new(vec![]))),
        ToolOperator::new(working_dir),
    )
}

#[test]
fn undo_stack_push_pop_lifo_and_eviction() {
    let mut mgr = make_mgr(std::env::temp_dir());
    assert_eq!(mgr.undo_stack_len(), 0);
    assert!(mgr.pop_undo_checkpoint().is_none());

    for i in 0..3 {
        mgr.push_undo_checkpoint(UndoCheckpoint {
            tool_name: format!("tool_{i}"),
            path: PathBuf::from(format!("/tmp/{i}.txt")),
            cleanup_path: None,
            previous_content: None,
        });
    }
    assert_eq!(mgr.undo_stack_len(), 3);
    assert_eq!(mgr.pop_undo_checkpoint().unwrap().tool_name, "tool_2");
    assert_eq!(mgr.pop_undo_checkpoint().unwrap().tool_name, "tool_1");
    assert_eq!(mgr.pop_undo_checkpoint().unwrap().tool_name, "tool_0");
    assert!(mgr.pop_undo_checkpoint().is_none());
}

#[test]
fn undo_checkpoint_captures_and_restores_file_content() {
    let temp = tempfile::tempdir().unwrap();
    let file_path = temp.path().join("file.txt");
    std::fs::write(&file_path, "original content").unwrap();

    let mut mgr = make_mgr(temp.path().to_path_buf());
    let snapshot =
        mgr.capture_undo_snapshot("write_file", &json!({"path": file_path.to_str().unwrap()}));
    assert!(snapshot.is_some());
    let cp = snapshot.unwrap();
    assert_eq!(cp.previous_content, Some(b"original content".to_vec()));
    assert_eq!(cp.tool_name, "write_file");
}

#[test]
fn zero_max_checkpoints_disables_undo() {
    let mut mgr = make_mgr(std::env::temp_dir());
    mgr.max_undo_checkpoints = 0;
    mgr.push_undo_checkpoint(UndoCheckpoint {
        tool_name: "write_file".to_string(),
        path: PathBuf::from("/tmp/x.txt"),
        cleanup_path: None,
        previous_content: None,
    });
    assert_eq!(mgr.undo_stack_len(), 0);
}
