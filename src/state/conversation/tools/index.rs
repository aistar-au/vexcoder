use crate::config::SearchConfig;
use crate::tools::index::{self, IndexChunk};
use std::sync::{Mutex, OnceLock};

/// Lazily-built structural index for the codebase_search tool.
pub(super) static CODEBASE_INDEX: OnceLock<Mutex<Vec<IndexChunk>>> = OnceLock::new();

pub(super) fn build_codebase_index(
    workspace_root: &std::path::Path,
    search_config: &SearchConfig,
) -> Vec<IndexChunk> {
    index::build_index_filtered(
        workspace_root,
        &search_config.exclude,
        search_config.max_file_size,
    )
}

/// Refresh the structural index for a single changed file (if the index exists).
pub(super) fn refresh_codebase_index(
    rel_path: &str,
    workspace_root: &std::path::Path,
    search_config: &SearchConfig,
) {
    if !search_config.enabled {
        return;
    }

    if let Some(idx_mutex) = CODEBASE_INDEX.get() {
        if let Ok(mut idx) = idx_mutex.lock() {
            let abs_path = workspace_root.join(rel_path);
            index::update_index_filtered(
                &mut idx,
                &abs_path,
                workspace_root,
                &search_config.exclude,
                search_config.max_file_size,
            );
        }
    }
}

#[cfg(test)]
pub(crate) fn rebuild_codebase_index_for_tests(workspace_root: &std::path::Path) {
    let idx_mutex = CODEBASE_INDEX.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut idx) = idx_mutex.lock() {
        *idx = index::build_index(workspace_root);
    }
}

#[cfg(test)]
pub(crate) fn clear_codebase_index_for_tests() {
    let idx_mutex = CODEBASE_INDEX.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut idx) = idx_mutex.lock() {
        idx.clear();
    }
}

#[cfg(test)]
pub(crate) fn codebase_index_names_for_tests() -> Vec<String> {
    let idx_mutex = CODEBASE_INDEX.get_or_init(|| Mutex::new(Vec::new()));
    idx_mutex
        .lock()
        .map(|idx| idx.iter().map(|chunk| chunk.name.clone()).collect())
        .unwrap_or_default()
}

pub(super) fn rebuild_codebase_index_with_config(
    workspace_root: &std::path::Path,
    exclude: &[String],
    max_file_size: usize,
) -> usize {
    let idx_mutex = CODEBASE_INDEX.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut idx) = idx_mutex.lock() {
        *idx = index::build_index_filtered(workspace_root, exclude, max_file_size);
        return idx.len();
    }
    0
}

pub(crate) fn warm_codebase_index_with_config(
    workspace_root: &std::path::Path,
    search_config: &SearchConfig,
) -> Option<usize> {
    if !(search_config.enabled && search_config.auto_index) {
        return None;
    }

    Some(rebuild_codebase_index_with_config(
        workspace_root,
        &search_config.exclude,
        search_config.max_file_size,
    ))
}

/// Force a full rebuild of the structural codebase index.
/// Used by the `/reindex` slash command.
///
/// Pass `&[]` and `usize::MAX` to apply no filtering (equivalent to default
/// `SearchConfig` values).
pub(crate) fn force_full_reindex_with_config(
    workspace_root: &std::path::Path,
    exclude: &[String],
    max_file_size: usize,
) -> usize {
    rebuild_codebase_index_with_config(workspace_root, exclude, max_file_size)
}
