use anyhow::Result;
use std::path::Path;

use crate::runtime::{StateEnvelope, TaskState};

/// Load the versioned envelope for one parent task. Missing `{id}.json`
/// is `Ok(None)`. A present task whose sidecar fails to deserialize is `Err`.
#[tracing::instrument(skip(working_dir), fields(working_dir = %working_dir.display()))]
pub fn facade_working_set(working_dir: &Path, task_id: &str) -> Result<Option<StateEnvelope>> {
    for dir in TaskState::state_search_dirs_from(working_dir) {
        let task_path = dir.join(format!("{task_id}.json"));
        if task_path.is_file() {
            return StateEnvelope::load_for_task(&dir, task_id).map(Some);
        }
    }
    Ok(None)
}
