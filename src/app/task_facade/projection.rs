use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::runtime::session_task::now_millis;
use crate::runtime::TaskState;
use crate::util::write_json_safe;

use super::{session_task_to_snapshot, FacadeSessionTaskSnapshot, FacadeTaskGraphNode};

// ---------------------------------------------------------------------------
// File names written inside the projections subdirectory.
// ---------------------------------------------------------------------------

/// Subdirectory under `.vex/state/` where projection snapshots are written.
/// Keeping them out of the root state dir prevents `state_files_in_dir` from
/// treating them as task-state files and failing deserialization.
const PROJECTIONS_SUBDIR: &str = "projections";
const TASK_GRAPH_FILE: &str = "task-graph.json";
const TODOS_FILE: &str = "todos.json";

// ---------------------------------------------------------------------------
// On-disk serialisable projection types
// ---------------------------------------------------------------------------

/// Serialisable snapshot of the full task graph, written to
/// `.vex/state/projections/task-graph.json` after every facade mutation.
#[derive(Debug, Serialize)]
pub struct TaskGraphSnapshot {
    pub generated_at_ms: u64,
    pub nodes: Vec<FacadeTaskGraphNode>,
}

/// Serialisable snapshot of all live session tasks, written to
/// `.vex/state/projections/todos.json` after every facade mutation.
#[derive(Debug, Serialize)]
pub struct TodosSnapshot {
    pub generated_at_ms: u64,
    pub items: Vec<TodoSnapshotItem>,
}

/// One item in the todos projection file.
#[derive(Debug, Serialize)]
pub struct TodoSnapshotItem {
    pub id: String,
    pub parent_task_id: String,
    pub agent_id: String,
    pub lifecycle_state: String,
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Absolute path to the task-graph projection file for the given working dir.
pub fn task_graph_snapshot_path(working_dir: &Path) -> PathBuf {
    TaskState::state_dir_from(working_dir)
        .join(PROJECTIONS_SUBDIR)
        .join(TASK_GRAPH_FILE)
}

/// Absolute path to the todos projection file for the given working dir.
pub fn todos_snapshot_path(working_dir: &Path) -> PathBuf {
    TaskState::state_dir_from(working_dir)
        .join(PROJECTIONS_SUBDIR)
        .join(TODOS_FILE)
}

// ---------------------------------------------------------------------------
// Snapshot writer
// ---------------------------------------------------------------------------

/// Build the task-graph and todos projections from the current persisted
/// task-state files and write them atomically to the state directory.
///
/// Failures are intentionally non-fatal at the call sites: the state files
/// remain the durable source of truth.  Call sites log a warning when this
/// returns an error.
pub fn write_projection_snapshot(working_dir: &Path) -> Result<()> {
    let now = now_millis();
    let projections_dir = TaskState::state_dir_from(working_dir).join(PROJECTIONS_SUBDIR);

    let mut graph_nodes: Vec<FacadeTaskGraphNode> = Vec::new();
    let mut todo_items: Vec<TodoSnapshotItem> = Vec::new();

    for file in TaskState::state_files_from(working_dir) {
        let state = TaskState::load(&file.dir, &file.id)?;

        let session_tasks: Vec<FacadeSessionTaskSnapshot> = state
            .session_tasks
            .iter()
            .map(|t| session_task_to_snapshot(t.clone()))
            .collect();

        for task in &state.session_tasks {
            if task.lifecycle_state.is_live() {
                todo_items.push(TodoSnapshotItem {
                    id: task.id.clone(),
                    parent_task_id: task.parent_task_id.clone(),
                    agent_id: task.agent_id.clone(),
                    lifecycle_state: task.lifecycle_state.to_string(),
                });
            }
        }

        graph_nodes.push(FacadeTaskGraphNode {
            id: state.id,
            status: state.status.to_string(),
            agent_id: state.agent_id,
            session_tasks,
        });
    }

    let graph_snapshot = TaskGraphSnapshot {
        generated_at_ms: now,
        nodes: graph_nodes,
    };
    let todos_snapshot = TodosSnapshot {
        generated_at_ms: now,
        items: todo_items,
    };

    write_json_safe(
        &projections_dir.join(TASK_GRAPH_FILE),
        &graph_snapshot,
        "task-graph projection",
    )?;
    write_json_safe(
        &projections_dir.join(TODOS_FILE),
        &todos_snapshot,
        "todos projection",
    )?;

    Ok(())
}
