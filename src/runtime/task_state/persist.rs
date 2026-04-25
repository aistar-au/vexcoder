use anyhow::{Context, Result, anyhow};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{TaskId, TaskState};
use crate::runtime::session_task::{SessionTask, now_millis};
use crate::pulse_evidence::normalize_tool_invocation_step_ids;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStateFile {
    pub dir: PathBuf,
    pub id: TaskId,
    pub modified_millis: u128,
}

fn state_file_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

fn insert_bounded_state_file(selected: &mut Vec<TaskStateFile>, file: TaskStateFile, cap: usize) {
    if cap == 0 {
        return;
    }

    if let Some(existing) = selected.iter_mut().find(|existing| existing.id == file.id) {
        if file.modified_millis > existing.modified_millis {
            *existing = file;
        }
        return;
    }

    if selected.len() < cap {
        selected.push(file);
        return;
    }

    let Some((oldest_index, oldest)) = selected
        .iter()
        .enumerate()
        .min_by_key(|(_, existing)| existing.modified_millis)
    else {
        return;
    };

    if file.modified_millis > oldest.modified_millis {
        selected[oldest_index] = file;
    }
}

#[cfg(feature = "startup-tracing")]
fn emit_startup_scan_trace(scanned_files: usize, cap: usize, selected: &[TaskStateFile]) {
    if !crate::config::StartupBudget::default().trace_allocations {
        return;
    }

    let approx_bytes = selected
        .iter()
        .map(|file| {
            std::mem::size_of::<TaskStateFile>() + file.id.len() + file.dir.as_os_str().len()
        })
        .sum::<usize>();

    eprintln!(
        "[startup-alloc] scanned_files={scanned_files} selected_files={} scan_cap={cap} approx_task_state_file_bytes={approx_bytes}",
        selected.len()
    );
}

#[cfg(not(feature = "startup-tracing"))]
fn emit_startup_scan_trace(_scanned_files: usize, _cap: usize, _selected: &[TaskStateFile]) {}

fn visit_state_files_in_dir(dir: &Path, mut visit: impl FnMut(TaskStateFile)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(id) = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.to_string())
        else {
            continue;
        };
        let modified_millis = entry
            .metadata()
            .ok()
            .and_then(|info| info.modified().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        visit(TaskStateFile {
            dir: dir.to_path_buf(),
            id,
            modified_millis,
        });
    }
}

fn write_pretty_json_safe(path: &Path, value: &TaskState, label: &str) -> Result<()> {
    crate::util::write_json_safe(path, value, label)
}

impl TaskState {
    pub fn save(&self, dir: &Path) -> Result<()> {
        let final_path = state_file_path(dir, &self.id);
        crate::tools::operator::policy::assert_durable_access(&final_path)?;
        write_pretty_json_safe(&final_path, self, "task state")?;
        Ok(())
    }

    pub fn load(dir: &Path, id: &str) -> Result<Self> {
        let path = state_file_path(dir, id);
        crate::tools::operator::policy::assert_durable_access(&path)?;
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read state file: {}", path.display()))?;

        let mut state: TaskState = serde_json::from_str(&content)
            .with_context(|| format!("Failed to deserialize state file: {}", path.display()))?;

        for evidence in &mut state.command_history {
            if evidence.exit_code.is_none() {
                evidence.interrupted = true;
            }
        }

        if state.updated_at == 0 {
            state.updated_at = now_millis();
        }

        normalize_tool_invocation_step_ids(&mut state.pulses);

        Ok(state)
    }

    pub fn state_dir() -> PathBuf {
        let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::state_dir_from(&working_dir)
    }

    pub fn state_dir_from(working_dir: &Path) -> PathBuf {
        match std::env::var("VEX_STATE_DIR") {
            Ok(path) => crate::workspace::resolve_relative_to_workspace(working_dir, path.into()),
            Err(_) => crate::workspace::workspace_root(working_dir).join(".vex/state"),
        }
    }

    pub fn state_search_dirs() -> Vec<PathBuf> {
        let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::state_search_dirs_from(&working_dir)
    }

    pub fn state_search_dirs_from(working_dir: &Path) -> Vec<PathBuf> {
        let mut dirs = vec![Self::state_dir_from(working_dir)];
        let legacy = match std::env::var("VEX_STATE_DIR") {
            Ok(path) => {
                let path = PathBuf::from(path);
                (!path.is_absolute()).then(|| working_dir.join(path))
            }
            Err(_) => Some(working_dir.join(".vex/state")),
        };

        if let Some(legacy) = legacy
            && !dirs.iter().any(|dir| dir == &legacy)
        {
            dirs.push(legacy);
        }

        dirs
    }

    pub fn state_files() -> Vec<TaskStateFile> {
        let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::state_files_from(&working_dir)
    }

    pub fn state_files_from(working_dir: &Path) -> Vec<TaskStateFile> {
        Self::state_files_from_with_limit(
            working_dir,
            Some(crate::config::StartupBudget::default().max_scans),
        )
    }

    pub fn state_files_from_with_limit(
        working_dir: &Path,
        limit: Option<usize>,
    ) -> Vec<TaskStateFile> {
        let cap = limit.unwrap_or_else(|| crate::config::StartupBudget::default().max_scans);
        let mut files = Vec::with_capacity(cap);
        let mut scanned_files = 0usize;
        for dir in Self::state_search_dirs_from(working_dir) {
            visit_state_files_in_dir(&dir, |file| {
                scanned_files = scanned_files.saturating_add(1);
                insert_bounded_state_file(&mut files, file, cap);
            });
        }
        files.sort_by_key(|file| std::cmp::Reverse(file.modified_millis));
        emit_startup_scan_trace(scanned_files, cap, &files);
        files
    }

    pub fn load_from_search_dirs(id: &str) -> Result<Self> {
        let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::load_from_search_dirs_from(&working_dir, id)
    }

    pub fn load_from_search_dirs_from(working_dir: &Path, id: &str) -> Result<Self> {
        if let Some(file) = Self::state_files_from(working_dir)
            .into_iter()
            .find(|file| file.id == id)
        {
            return Self::load(&file.dir, id);
        }

        Err(anyhow!("task state '{id}' not found"))
    }

    pub fn find_session_task_in_saved_states(
        working_dir: &Path,
        session_task_id: &str,
    ) -> Result<Option<(TaskState, SessionTask)>> {
        use super::header_cache::cached_task_header;
        use crate::config::StartupBudget;

        let budget = StartupBudget::default();

        for file in Self::state_files_from_with_limit(working_dir, Some(budget.max_scans)) {
            let path = file.dir.join(format!("{}.json", file.id));

            let header = match cached_task_header(&path) {
                Some(h) => h,
                None => {
                    tracing::debug!(
                        task_id = %file.id,
                        path = %path.display(),
                        "skipping unreadable task state during session task search"
                    );
                    continue;
                }
            };

            let is_candidate = match &header.session_tasks {
                None => true,
                Some(tasks) => tasks.iter().any(|t| {
                    session_task_id.starts_with(&format!("{}-{}-", header.id, t.agent_id))
                }),
            };

            if !is_candidate {
                continue;
            }

            let state = Self::load(&file.dir, &file.id)?;
            if let Some(task) = state.session_task(session_task_id).cloned() {
                return Ok(Some((state, task)));
            }
        }
        Ok(None)
    }

    pub fn live_session_task_counts_from(working_dir: &Path) -> Result<HashMap<String, usize>> {
        use super::header_cache::cached_task_header;
        use crate::config::StartupBudget;

        let budget = StartupBudget::default();
        let mut counts = HashMap::new();

        for file in Self::state_files_from_with_limit(working_dir, Some(budget.max_scans)) {
            let path = file.dir.join(format!("{}.json", file.id));
            match cached_task_header(&path) {
                Some(header) => {
                    for task in header.session_tasks.unwrap_or_default() {
                        if task.status.is_live() {
                            *counts.entry(task.agent_id).or_default() += 1;
                        }
                    }
                }
                None => tracing::debug!(
                    task_id = %file.id,
                    path = %path.display(),
                    "skipping unreadable task state during inline active agent scan"
                ),
            }
        }
        Ok(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::approval::{ApprovalScope, Capability};
    use crate::runtime::task_state::{
        CacheUsageStats, CommandEvidence, ContextCompactionRecord, ConversationCheckpoint,
        InterruptedCommand, SessionNote, TaskStatus,
    };
    use crate::test_support::ENV_LOCK;
    use crate::pulse_evidence::TurnEvidenceState;
    use tempfile::TempDir;

    #[test]
    fn task_state_survives_atomic_write_and_reload() {
        let dir = TempDir::new().unwrap();
        let state = TaskState {
            id: "task-001".to_string(),
            status: TaskStatus::Completed,
            parent_task_id: Some("parent-root".to_string()),
            agent_id: Some("reviewer".to_string()),
            worktree_path: Some(PathBuf::from(".vex/state/worktrees/task-001")),
            started_at: Some(123),
            updated_at: 456,
            last_heartbeat: Some(789),
            handoff_summary: Some("session summary".to_string()),
            active_grants: HashMap::from([(Capability::ApplyPatch, ApprovalScope::Once)]),
            changed_files: vec![PathBuf::from("src/main.rs")],
            command_history: vec![CommandEvidence {
                program: "cargo test".into(),
                exit_code: None,
                interrupted: true,
            }],
            conversation_snapshot: ConversationCheckpoint::default(),
            interrupted_sessions: vec![InterruptedCommand {
                program: "cargo build".into(),
                interrupted_at: "2026-03-01T00:00:00Z".into(),
            }],
            branch_name: Some("feature/task-001".to_string()),
            instructions_path: Some("AGENTS.md".to_string()),
            pulses: vec![TurnEvidenceState {
                input: "explain the error".to_string(),
                response: "world".to_string(),
                changed_files: vec!["src/main.rs".to_string()],
                command_history: Vec::new(),
                tool_invocations: Vec::new(),
                tokens: Default::default(),
            }],
            plan: Some("step 1: do the thing".to_string()),
            session_notes: vec![SessionNote {
                content: "remember this".to_string(),
                created_at_turn: 0,
            }],
            context_compaction: vec![ContextCompactionRecord {
                turn_index: 1,
                messages_before: 20,
                messages_after: 4,
                summary: "trimmed early pulses".to_string(),
            }],
            cache_usage: CacheUsageStats {
                total_cache_creation_tokens: 500,
                total_cache_read_tokens: 1200,
            },
            session_tasks: vec![SessionTask::new(
                "task-001",
                "reviewer",
                "inspect task-state",
                Some(PathBuf::from(".vex/state/worktrees/task-001-reviewer")),
            )],
        };

        state.save(dir.path()).expect("save failed");
        let loaded = TaskState::load(dir.path(), "task-001").expect("load failed");

        assert_eq!(loaded.status, TaskStatus::Completed);
        assert_eq!(loaded.changed_files, state.changed_files);
        assert!(loaded.command_history[0].interrupted);
        assert_eq!(loaded.interrupted_sessions.len(), 1);
        assert_eq!(loaded.branch_name, state.branch_name);
        assert_eq!(loaded.instructions_path, state.instructions_path);
        assert_eq!(loaded.pulses, state.pulses);
        assert_eq!(loaded.plan, state.plan);
        assert_eq!(loaded.session_notes, state.session_notes);
        assert_eq!(loaded.context_compaction, state.context_compaction);
        assert_eq!(loaded.cache_usage, state.cache_usage);
        assert_eq!(loaded.parent_task_id, state.parent_task_id);
        assert_eq!(loaded.agent_id, state.agent_id);
        assert_eq!(loaded.worktree_path, state.worktree_path);
        assert_eq!(loaded.handoff_summary, state.handoff_summary);
        assert_eq!(loaded.session_tasks.len(), 1);
    }

    #[test]
    fn pre_adr029_file_loads_with_default_new_fields() {
        let dir = TempDir::new().unwrap();
        let legacy_json = r#"{
            "id": "task-legacy",
            "status": "Completed",
            "active_grants": {},
            "changed_files": [],
            "command_history": [],
            "conversation_snapshot": {"message_count": 0, "summary": ""},
            "interrupted_sessions": []
        }"#;
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.path().join("task-legacy.json"), legacy_json).unwrap();

        let loaded = TaskState::load(dir.path(), "task-legacy").expect("load failed");
        assert_eq!(loaded.plan, None);
        assert!(loaded.session_notes.is_empty());
        assert!(loaded.context_compaction.is_empty());
        assert_eq!(loaded.cache_usage, CacheUsageStats::default());
        assert!(loaded.session_tasks.is_empty());
        assert_eq!(loaded.parent_task_id, None);
    }

    #[test]
    fn load_backfills_missing_tool_invocation_step_ids() {
        let dir = TempDir::new().unwrap();
        let legacy_json = r#"{
            "id": "task-step-legacy",
            "status": "Completed",
            "active_grants": {},
            "changed_files": [],
            "command_history": [],
            "conversation_snapshot": {"message_count": 0, "summary": ""},
            "interrupted_sessions": [],
            "pulses": [
                {
                    "input": "hi",
                    "response": "done",
                    "changed_files": [],
                    "command_history": [],
                    "tool_invocations": [
                        {"name": "read_file", "outcome": "ok"},
                        {"step_id": 2, "name": "edit_file", "outcome": "ok"},
                        {"step_id": 2, "name": "run_command", "outcome": "ok"}
                    ],
                    "tokens": {"input": 0, "output": 0}
                }
            ]
        }"#;
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.path().join("task-step-legacy.json"), legacy_json).unwrap();

        let loaded = TaskState::load(dir.path(), "task-step-legacy").expect("load failed");
        let step_ids = loaded.pulses[0]
            .tool_invocations
            .iter()
            .map(|invocation| invocation.step_id)
            .collect::<Vec<_>>();

        assert_eq!(step_ids, vec![1, 2, 3]);
    }

    #[test]
    fn marks_interrupted_commands_on_reload() {
        let dir = TempDir::new().unwrap();
        let state = TaskState {
            id: "task-456".to_string(),
            status: TaskStatus::Running,
            parent_task_id: None,
            agent_id: None,
            worktree_path: None,
            started_at: Some(1),
            updated_at: 2,
            last_heartbeat: None,
            handoff_summary: None,
            active_grants: HashMap::new(),
            changed_files: Vec::new(),
            command_history: vec![CommandEvidence {
                program: "sleep 100".to_string(),
                exit_code: None,
                interrupted: false,
            }],
            conversation_snapshot: ConversationCheckpoint::default(),
            interrupted_sessions: Vec::new(),
            branch_name: None,
            instructions_path: None,
            pulses: Vec::new(),
            plan: None,
            session_notes: Vec::new(),
            context_compaction: Vec::new(),
            cache_usage: CacheUsageStats::default(),
            session_tasks: Vec::new(),
        };

        state.save(dir.path()).expect("save failed");
        let loaded = TaskState::load(dir.path(), "task-456").expect("load failed");
        assert_eq!(loaded.command_history.len(), 1);
        assert!(loaded.command_history[0].interrupted);
    }

    #[test]
    fn state_dir_defaults_to_repo_root_for_subdirs() {
        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();
        crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");

        assert_eq!(
            TaskState::state_dir_from(&nested),
            temp.path().join(".vex/state")
        );
    }

    #[test]
    fn state_dir_relative_env_is_anchored_to_repo_root() {
        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();
        crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", "custom/state");

        assert_eq!(
            TaskState::state_dir_from(&nested),
            temp.path().join("custom/state")
        );

        crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
    }

    #[test]
    fn state_dir_absolute_env_is_preserved() {
        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        let absolute = temp.path().join("absolute-state");
        crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", absolute.as_os_str());

        assert_eq!(TaskState::state_dir_from(temp.path()), absolute);

        crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
    }

    #[test]
    fn state_search_dirs_include_legacy_subdir_path() {
        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            TaskState::state_search_dirs_from(&nested),
            vec![temp.path().join(".vex/state"), nested.join(".vex/state")]
        );
    }

    #[test]
    fn state_search_dirs_include_legacy_relative_env_path() {
        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();
        crate::test_support::test_set_var(&_env_lock, "VEX_STATE_DIR", "custom/state");

        assert_eq!(
            TaskState::state_search_dirs_from(&nested),
            vec![
                temp.path().join("custom/state"),
                nested.join("custom/state")
            ]
        );

        crate::test_support::test_remove_var(&_env_lock, "VEX_STATE_DIR");
    }

    #[test]
    fn live_session_task_counts_from_reads_inline_state_summary() {
        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let state_dir = TaskState::state_dir_from(temp.path());

        let mut state = TaskState::new("task-live".to_string());
        state.add_session_task(SessionTask::new(
            "task-live",
            "reviewer",
            "review docs",
            None,
        ));
        state.save(&state_dir).unwrap();

        let counts = TaskState::live_session_task_counts_from(temp.path()).unwrap();
        assert_eq!(counts.get("reviewer"), Some(&1));
    }

    #[test]
    fn state_files_prefer_newest_copy_of_duplicate_task_ids() {
        use filetime::{FileTime, set_file_mtime};

        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        let repo_state_dir = temp.path().join(".vex/state");
        let legacy_state_dir = nested.join(".vex/state");
        std::fs::create_dir_all(&legacy_state_dir).unwrap();

        let mut legacy = TaskState::new("task-dup".to_string());
        legacy.status = TaskStatus::Running;
        legacy.save(&legacy_state_dir).unwrap();
        set_file_mtime(
            legacy_state_dir.join("task-dup.json"),
            FileTime::from_unix_time(1_700_000_002, 0),
        )
        .unwrap();

        std::fs::create_dir_all(&repo_state_dir).unwrap();
        TaskState::new("task-dup".to_string())
            .save(&repo_state_dir)
            .unwrap();
        set_file_mtime(
            repo_state_dir.join("task-dup.json"),
            FileTime::from_unix_time(1_700_000_001, 0),
        )
        .unwrap();

        let files = TaskState::state_files_from(&nested);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].dir, legacy_state_dir);
    }

    #[test]
    fn live_session_task_counts_prefer_newest_copy_of_duplicate_task_ids() {
        use filetime::{FileTime, set_file_mtime};

        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        let repo_state_dir = temp.path().join(".vex/state");
        let legacy_state_dir = nested.join(".vex/state");
        std::fs::create_dir_all(&legacy_state_dir).unwrap();

        let mut legacy = TaskState::new("task-dup".to_string());
        legacy.add_session_task(SessionTask::new(
            "task-dup",
            "legacy-reviewer",
            "review docs",
            None,
        ));
        legacy.save(&legacy_state_dir).unwrap();
        set_file_mtime(
            legacy_state_dir.join("task-dup.json"),
            FileTime::from_unix_time(1_700_000_002, 0),
        )
        .unwrap();

        std::fs::create_dir_all(&repo_state_dir).unwrap();
        let mut repo = TaskState::new("task-dup".to_string());
        repo.add_session_task(SessionTask::new(
            "task-dup",
            "repo-reviewer",
            "review docs",
            None,
        ));
        repo.save(&repo_state_dir).unwrap();
        set_file_mtime(
            repo_state_dir.join("task-dup.json"),
            FileTime::from_unix_time(1_700_000_001, 0),
        )
        .unwrap();

        let counts = TaskState::live_session_task_counts_from(&nested).unwrap();
        assert_eq!(counts.get("legacy-reviewer"), Some(&1));
        assert!(!counts.contains_key("repo-reviewer"));
    }

    #[test]
    fn state_files_from_with_limit_returns_newest_n() {
        use filetime::{FileTime, set_file_mtime};
        let _guard = ENV_LOCK.blocking_lock();
        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join(".vex/state");
        std::fs::create_dir_all(&state_dir).unwrap();

        for i in 0..5u32 {
            let id = format!("scan-task-{i}");
            let state = TaskState::new(id.clone());
            state.save(&state_dir).unwrap();
            set_file_mtime(
                state_dir.join(format!("{id}.json")),
                FileTime::from_unix_time(1_700_000_000 + i as i64, 0),
            )
            .unwrap();
        }

        crate::test_support::test_set_var(&_guard, "VEX_STATE_DIR", state_dir.to_str().unwrap());
        let files = TaskState::state_files_from_with_limit(dir.path(), Some(3));
        crate::test_support::test_remove_var(&_guard, "VEX_STATE_DIR");

        assert_eq!(files.len(), 3);

        assert_eq!(files[0].id, "scan-task-4");
        assert_eq!(files[1].id, "scan-task-3");
        assert_eq!(files[2].id, "scan-task-2");
    }

    #[test]
    fn state_files_from_with_none_limit_applies_default_cap() {
        let _guard = ENV_LOCK.blocking_lock();
        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join(".vex/state");
        std::fs::create_dir_all(&state_dir).unwrap();

        for i in 0..4u32 {
            let id = format!("compat-{i}");
            let state = TaskState::new(id.clone());
            state.save(&state_dir).unwrap();
        }

        crate::test_support::test_set_var(&_guard, "VEX_STATE_DIR", state_dir.to_str().unwrap());
        let with_none = TaskState::state_files_from_with_limit(dir.path(), None);
        let with_explicit = TaskState::state_files_from(dir.path());
        crate::test_support::test_remove_var(&_guard, "VEX_STATE_DIR");

        assert_eq!(with_none.len(), with_explicit.len());
        assert_eq!(with_none.len(), 4);
    }

    #[test]
    fn find_session_task_handles_legacy_json_without_session_tasks() {
        let _guard = ENV_LOCK.blocking_lock();
        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join(".vex/state");
        std::fs::create_dir_all(&state_dir).unwrap();

        let mut root = TaskState::new("legacy-root".to_string());
        let session_task = SessionTask::new("legacy-root", "bot", "do work", None);
        let expected_id = session_task.id.clone();
        root.add_session_task(session_task);
        root.save(&state_dir).unwrap();

        crate::test_support::test_set_var(&_guard, "VEX_STATE_DIR", state_dir.to_str().unwrap());
        let result =
            TaskState::find_session_task_in_saved_states(dir.path(), &expected_id).unwrap();
        crate::test_support::test_remove_var(&_guard, "VEX_STATE_DIR");

        assert!(result.is_some());
        let (_, found) = result.unwrap();
        assert_eq!(found.id, expected_id);
    }

    #[test]
    fn live_session_task_counts_uses_header_only() {
        let _guard = ENV_LOCK.blocking_lock();
        let dir = TempDir::new().unwrap();
        let state_dir = dir.path().join(".vex/state");
        std::fs::create_dir_all(&state_dir).unwrap();

        let mut root = TaskState::new("hdr-task".to_string());
        root.add_session_task(SessionTask::new("hdr-task", "analyst", "analyse", None));
        root.save(&state_dir).unwrap();

        crate::test_support::test_set_var(&_guard, "VEX_STATE_DIR", state_dir.to_str().unwrap());
        let counts = TaskState::live_session_task_counts_from(dir.path()).unwrap();
        crate::test_support::test_remove_var(&_guard, "VEX_STATE_DIR");

        assert_eq!(counts.get("analyst"), Some(&1));
    }

    #[test]
    fn state_files_from_with_limit_prefers_newest_duplicate_copy() {
        use filetime::{FileTime, set_file_mtime};

        let _guard = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        let repo_state_dir = temp.path().join(".vex/state");
        let legacy_state_dir = nested.join(".vex/state");
        std::fs::create_dir_all(&legacy_state_dir).unwrap();

        let legacy = TaskState::new("task-dup".to_string());
        legacy.save(&legacy_state_dir).unwrap();
        set_file_mtime(
            legacy_state_dir.join("task-dup.json"),
            FileTime::from_unix_time(1_700_000_002, 0),
        )
        .unwrap();

        std::fs::create_dir_all(&repo_state_dir).unwrap();
        TaskState::new("task-dup".to_string())
            .save(&repo_state_dir)
            .unwrap();
        set_file_mtime(
            repo_state_dir.join("task-dup.json"),
            FileTime::from_unix_time(1_700_000_001, 0),
        )
        .unwrap();

        let files = TaskState::state_files_from_with_limit(&nested, Some(1));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].dir, legacy_state_dir);
    }
}
