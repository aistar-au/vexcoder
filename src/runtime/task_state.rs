use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::runtime::session_task::{now_millis, SessionTask, SessionTaskStatus};
use crate::runtime::{ApprovalScope, Capability};
use crate::turn_evidence::{normalize_tool_invocation_step_ids, TurnEvidenceState};

pub type TaskId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Ready,
    Running,
    AwaitingApproval,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    /// Headless batch run stopped because `--max-turns` was reached before the
    /// task completed. Distinct from `Completed` so CI can treat it as failure.
    MaxTurnsReached,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandEvidence {
    pub program: String,
    pub exit_code: Option<i32>,
    pub interrupted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationCheckpoint {
    pub message_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptedCommand {
    pub program: String,
    pub interrupted_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionNote {
    pub content: String,
    pub created_at_turn: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextCompactionRecord {
    pub turn_index: usize,
    pub messages_before: usize,
    pub messages_after: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheUsageStats {
    pub total_cache_creation_tokens: u64,
    pub total_cache_read_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskState {
    pub id: TaskId,
    pub status: TaskStatus,
    #[serde(default)]
    pub parent_task_id: Option<TaskId>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_summary: Option<String>,
    pub active_grants: HashMap<Capability, ApprovalScope>,
    pub changed_files: Vec<PathBuf>,
    pub command_history: Vec<CommandEvidence>,
    pub conversation_snapshot: ConversationCheckpoint,
    pub interrupted_sessions: Vec<InterruptedCommand>,
    #[serde(default)]
    pub branch_name: Option<String>,
    #[serde(default)]
    pub instructions_path: Option<String>,
    #[serde(default)]
    pub turns: Vec<TurnEvidenceState>,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub session_notes: Vec<SessionNote>,
    #[serde(default)]
    pub context_compaction: Vec<ContextCompactionRecord>,
    #[serde(default)]
    pub cache_usage: CacheUsageStats,
    #[serde(default, alias = "child_tasks")]
    pub session_tasks: Vec<SessionTask>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStateFile {
    pub dir: PathBuf,
    pub id: TaskId,
    pub modified_millis: u128,
}

impl TaskState {
    pub fn new(id: TaskId) -> Self {
        let now = now_millis();
        Self {
            id,
            status: TaskStatus::Ready,
            parent_task_id: None,
            agent_id: None,
            worktree_path: None,
            started_at: Some(now),
            updated_at: now,
            last_heartbeat: None,
            handoff_summary: None,
            active_grants: HashMap::new(),
            changed_files: Vec::new(),
            command_history: Vec::new(),
            conversation_snapshot: ConversationCheckpoint::default(),
            interrupted_sessions: Vec::new(),
            branch_name: None,
            instructions_path: None,
            turns: Vec::new(),
            plan: None,
            session_notes: Vec::new(),
            context_compaction: Vec::new(),
            cache_usage: CacheUsageStats::default(),
            session_tasks: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = now_millis();
    }

    pub fn record_heartbeat(&mut self) {
        let now = now_millis();
        self.last_heartbeat = Some(now);
        self.updated_at = now;
    }

    pub fn add_session_task(&mut self, session_task: SessionTask) {
        self.session_tasks.push(session_task);
        self.touch();
    }

    pub fn session_task(&self, id: &str) -> Option<&SessionTask> {
        self.session_tasks.iter().find(|task| task.id == id)
    }

    pub fn session_task_mut(&mut self, id: &str) -> Option<&mut SessionTask> {
        self.session_tasks.iter_mut().find(|task| task.id == id)
    }

    pub fn update_session_task_status(&mut self, id: &str, status: SessionTaskStatus) -> bool {
        let Some(task) = self.session_task_mut(id) else {
            return false;
        };
        task.transition_to(status);
        self.touch();
        true
    }

    pub fn find_session_task_in_saved_states(
        working_dir: &Path,
        session_task_id: &str,
    ) -> Result<Option<(TaskState, SessionTask)>> {
        for file in Self::state_files_from(working_dir) {
            let state = Self::load(&file.dir, &file.id)?;
            if let Some(task) = state.session_task(session_task_id).cloned() {
                return Ok(Some((state, task)));
            }
        }
        Ok(None)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create state directory: {}", dir.display()))?;

        let temp_path = dir.join(format!("{}.tmp", self.id));
        let final_path = dir.join(format!("{}.json", self.id));

        let json = serde_json::to_vec_pretty(self).context("Failed to serialize task state")?;
        let mut file = std::fs::File::create(&temp_path).with_context(|| {
            format!("Failed to create temp state file: {}", temp_path.display())
        })?;
        file.write_all(&json)
            .with_context(|| format!("Failed to write temp state file: {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("Failed to flush temp state file: {}", temp_path.display()))?;
        drop(file);

        std::fs::rename(&temp_path, &final_path)
            .with_context(|| format!("Failed to rename state file to: {}", final_path.display()))?;

        Ok(())
    }

    pub fn load(dir: &Path, id: &str) -> Result<Self> {
        let path = dir.join(format!("{}.json", id));
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

        normalize_tool_invocation_step_ids(&mut state.turns);

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

        if let Some(legacy) = legacy {
            if !dirs.iter().any(|dir| dir == &legacy) {
                dirs.push(legacy);
            }
        }

        dirs
    }

    pub fn state_files() -> Vec<TaskStateFile> {
        let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::state_files_from(&working_dir)
    }

    pub fn state_files_from(working_dir: &Path) -> Vec<TaskStateFile> {
        let mut files = Self::state_search_dirs_from(working_dir)
            .into_iter()
            .flat_map(|dir| {
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    return Vec::new();
                };

                entries
                    .filter_map(|entry| entry.ok())
                    .filter_map(|entry| {
                        let path = entry.path();
                        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                            return None;
                        }
                        let id = path.file_stem()?.to_str()?.to_string();
                        let modified_millis = entry
                            .metadata()
                            .ok()
                            .and_then(|meta| meta.modified().ok())
                            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|duration| duration.as_millis())
                            .unwrap_or(0);
                        Some(TaskStateFile {
                            dir: dir.clone(),
                            id,
                            modified_millis,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        files.sort_by(|left, right| right.modified_millis.cmp(&left.modified_millis));
        let mut seen = HashSet::new();
        files.retain(|file| seen.insert(file.id.clone()));
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ENV_LOCK;
    use tempfile::TempDir;

    #[test]
    fn test_task_state_survives_atomic_write_and_reload() {
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
            handoff_summary: Some("child summary".to_string()),
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
            turns: vec![TurnEvidenceState {
                input: "hello".to_string(),
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
                summary: "trimmed early turns".to_string(),
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
        assert_eq!(loaded.turns, state.turns);
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
    fn test_task_state_pre_adr029_file_loads_with_default_new_fields() {
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
    fn test_task_state_load_backfills_missing_tool_invocation_step_ids() {
        let dir = TempDir::new().unwrap();
        let legacy_json = r#"{
            "id": "task-step-legacy",
            "status": "Completed",
            "active_grants": {},
            "changed_files": [],
            "command_history": [],
            "conversation_snapshot": {"message_count": 0, "summary": ""},
            "interrupted_sessions": [],
            "turns": [
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
        let step_ids = loaded.turns[0]
            .tool_invocations
            .iter()
            .map(|invocation| invocation.step_id)
            .collect::<Vec<_>>();

        assert_eq!(step_ids, vec![1, 2, 3]);
    }

    #[test]
    fn test_cache_usage_stats_accumulate() {
        let mut stats = CacheUsageStats::default();
        stats.total_cache_creation_tokens += 100;
        stats.total_cache_read_tokens += 400;
        stats.total_cache_creation_tokens += 50;
        stats.total_cache_read_tokens += 600;
        assert_eq!(stats.total_cache_creation_tokens, 150);
        assert_eq!(stats.total_cache_read_tokens, 1000);
    }

    #[test]
    fn test_task_state_marks_interrupted_commands_on_reload() {
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
            turns: Vec::new(),
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
    fn test_max_turns_reached_is_distinct_from_completed() {
        assert_ne!(TaskStatus::MaxTurnsReached, TaskStatus::Completed);
        assert_ne!(TaskStatus::MaxTurnsReached, TaskStatus::Cancelled);
        assert_ne!(TaskStatus::MaxTurnsReached, TaskStatus::Failed);
    }

    #[test]
    fn test_state_dir_defaults_to_repo_root_for_subdirs() {
        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::env::remove_var("VEX_STATE_DIR");

        assert_eq!(
            TaskState::state_dir_from(&nested),
            temp.path().join(".vex/state")
        );
    }

    #[test]
    fn test_state_dir_relative_env_is_anchored_to_repo_root() {
        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::env::set_var("VEX_STATE_DIR", "custom/state");

        assert_eq!(
            TaskState::state_dir_from(&nested),
            temp.path().join("custom/state")
        );

        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_state_dir_absolute_env_is_preserved() {
        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        let absolute = temp.path().join("absolute-state");
        std::env::set_var("VEX_STATE_DIR", absolute.as_os_str());

        assert_eq!(TaskState::state_dir_from(temp.path()), absolute);

        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_state_search_dirs_include_legacy_subdir_path() {
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
    fn test_state_search_dirs_include_legacy_relative_env_path() {
        let _env_lock = ENV_LOCK.blocking_lock();
        let temp = TempDir::new().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        let nested = temp.path().join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::env::set_var("VEX_STATE_DIR", "custom/state");

        assert_eq!(
            TaskState::state_search_dirs_from(&nested),
            vec![
                temp.path().join("custom/state"),
                nested.join("custom/state")
            ]
        );

        std::env::remove_var("VEX_STATE_DIR");
    }

    #[test]
    fn test_state_files_prefer_newest_copy_of_duplicate_task_ids() {
        use filetime::{set_file_mtime, FileTime};

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
    fn test_add_and_update_session_task() {
        let mut state = TaskState::new("task-parent".to_string());
        let session_task = SessionTask::new(
            "task-parent",
            "docs-reviewer",
            "review docs",
            Some(PathBuf::from(
                ".vex/state/worktrees/task-parent-docs-reviewer",
            )),
        );
        let session_task_id = session_task.id.clone();

        state.add_session_task(session_task);
        assert_eq!(state.session_tasks.len(), 1);

        assert!(state.update_session_task_status(&session_task_id, SessionTaskStatus::Running));
        assert_eq!(
            state
                .session_task(&session_task_id)
                .map(|task| &task.lifecycle_state),
            Some(&SessionTaskStatus::Running)
        );
    }
}
