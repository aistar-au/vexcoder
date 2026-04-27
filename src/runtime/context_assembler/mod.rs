mod reads;

use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use self::reads::{extract_candidate_paths, infer_related_path_candidates, rollup_from_read};

use crate::runtime::context_cache::read_cached_file;
use crate::runtime::git_rollup::{collect_git_rollup, resolve_git_timeout_ms, watch_working_dir};
use crate::tools::ToolOperator;
use crate::util::parse_bool_flag;

const DEFAULT_MAX_FILE_BYTES: usize = 32_768;
const DEFAULT_MAX_DIFF_LINES: usize = 200;
const DEFAULT_MAX_RELATED: usize = 3;
const EXPANDED_MAX_RELATED: usize = 8;
const DEFAULT_GIT_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_INCLUDE_GIT_CONTEXT: bool = false;

#[derive(Debug, Clone)]
pub struct AssembledContext {
    pub file_rollups: Vec<FileRollup>,
    pub git_status_summary: Option<String>,
    pub recent_diff: Option<String>,

    pub has_staged_changes: bool,

    pub has_working_tree_changes: bool,

    pub git_dir: Option<PathBuf>,

    pub committer_name: Option<String>,

    pub staged_paths: Vec<PathBuf>,
    pub related_paths: Vec<PathBuf>,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

#[derive(Debug, Clone)]
pub struct FileRollup {
    pub path: PathBuf,
    pub content: Option<String>,
    pub content_limited: bool,
}

#[derive(Debug, Clone)]
pub struct ContextAssembler {
    pub max_file_bytes: usize,
    pub max_diff_lines: usize,
    pub max_related: usize,
    pub git_timeout_ms: u64,
    pub include_git_context: bool,
}

impl Default for ContextAssembler {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_diff_lines: DEFAULT_MAX_DIFF_LINES,
            max_related: DEFAULT_MAX_RELATED,
            git_timeout_ms: DEFAULT_GIT_TIMEOUT_MS,
            include_git_context: resolve_include_git_context(DEFAULT_INCLUDE_GIT_CONTEXT),
        }
    }
}

impl ContextAssembler {
    pub fn with_expanded_scan(mut self, enabled: bool) -> Self {
        if enabled {
            self.max_related = self.max_related.max(EXPANDED_MAX_RELATED);
        }
        self
    }

    pub fn with_git_context(mut self, enabled: bool) -> Self {
        self.include_git_context = enabled;
        self
    }

    pub fn watch_working_dir<F>(
        &self,
        working_dir: &Path,
        on_change: F,
    ) -> anyhow::Result<notify::RecommendedWatcher>
    where
        F: Fn(Vec<PathBuf>) + Send + 'static,
    {
        watch_working_dir(working_dir, on_change)
    }

    pub fn assemble(&self, instruction: &str, operator: &ToolOperator) -> Result<AssembledContext> {
        let timeout_ms = resolve_git_timeout_ms(self.git_timeout_ms);
        let mut file_rollups = Vec::new();
        let mut related_paths = Vec::new();
        let mut seen_paths = HashSet::new();
        let mut cache_hits = 0;
        let mut cache_misses = 0;

        for candidate in extract_candidate_paths(instruction) {
            let path = PathBuf::from(&candidate);
            if !seen_paths.insert(path.clone()) {
                continue;
            }
            let (snapshot, cache_hit) = rollup_from_read(
                path,
                read_cached_file(operator, &candidate),
                self.max_file_bytes,
            );
            if cache_hit {
                cache_hits += 1;
            } else if snapshot.content.is_some() {
                cache_misses += 1;
            }
            file_rollups.push(snapshot);
        }

        let named_rollup_count = file_rollups.len();
        for index in 0..named_rollup_count {
            if related_paths.len() >= self.max_related {
                break;
            }
            let Some(content) = file_rollups
                .get(index)
                .and_then(|snapshot| snapshot.content.as_deref())
            else {
                continue;
            };
            for inferred in infer_related_path_candidates(content) {
                if related_paths.len() >= self.max_related {
                    break;
                }
                if !seen_paths.insert(inferred.clone()) {
                    continue;
                }
                let candidate = inferred.to_string_lossy().replace('\\', "/");
                let Ok(read) = read_cached_file(operator, &candidate) else {
                    continue;
                };
                let (snapshot, cache_hit) =
                    rollup_from_read(inferred.clone(), Ok(read), self.max_file_bytes);
                if cache_hit {
                    cache_hits += 1;
                } else if snapshot.content.is_some() {
                    cache_misses += 1;
                }
                file_rollups.push(snapshot);
                related_paths.push(inferred);
            }
        }

        if !self.include_git_context {
            return Ok(AssembledContext {
                file_rollups,
                git_status_summary: None,
                recent_diff: None,
                has_staged_changes: false,
                has_working_tree_changes: false,
                git_dir: None,
                committer_name: None,
                staged_paths: Vec::new(),
                related_paths,
                cache_hits,
                cache_misses,
            });
        }

        let git_rollup = collect_git_rollup(
            operator.working_dir().to_path_buf(),
            timeout_ms,
            self.max_diff_lines,
        )?;
        let has_staged_changes = git_rollup.has_staged_changes();
        let has_working_tree_changes = git_rollup.has_working_tree_changes();

        Ok(AssembledContext {
            file_rollups,
            git_status_summary: git_rollup.git_status_summary,
            recent_diff: git_rollup.recent_diff,
            has_staged_changes,
            has_working_tree_changes,
            git_dir: git_rollup.git_dir,
            committer_name: git_rollup.committer_name,
            staged_paths: git_rollup.staged_paths,
            related_paths,
            cache_hits,
            cache_misses,
        })
    }

    pub fn render(&self, ctx: &AssembledContext) -> String {
        let mut out = String::new();
        out.push_str("## Context\n");

        self.render_file_rollups(ctx, &mut out);

        out.push_str("\n### Git status\n");
        match &ctx.git_status_summary {
            Some(summary) if !summary.trim().is_empty() => {
                out.push_str("```text\n");
                out.push_str(summary);
                if !summary.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("```\n");
            }
            _ => out.push_str("[context: unavailable]\n"),
        }

        out.push_str("\n### Recent diff\n");
        match &ctx.recent_diff {
            Some(diff) if !diff.trim().is_empty() => {
                out.push_str("```diff\n");
                out.push_str(diff);
                if !diff.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("```\n");
            }
            _ => out.push_str("[context: unavailable]\n"),
        }

        if !ctx.related_paths.is_empty() {
            out.push_str("\n### Related paths\n");
            for path in &ctx.related_paths {
                out.push_str(&format!("- {}\n", path.display()));
            }
        }

        out
    }

    pub fn render_shared_prefix(&self, ctx: &AssembledContext) -> String {
        let mut out = String::new();
        out.push_str("## Shared context prefix\n");

        self.render_file_rollups(ctx, &mut out);

        if !ctx.related_paths.is_empty() {
            out.push_str("\n### Related paths\n");
            for path in &ctx.related_paths {
                out.push_str(&format!("- {}\n", path.display()));
            }
        }

        out
    }

    fn render_file_rollups(&self, ctx: &AssembledContext, out: &mut String) {
        if ctx.file_rollups.is_empty() {
            out.push_str("[context: no file rollups]\n");
        } else {
            out.push_str("### File rollups\n");
            for snapshot in &ctx.file_rollups {
                out.push_str(&format!("- {}\n", snapshot.path.display()));
                if snapshot.content_limited {
                    out.push_str(&format!(
                        "  [context: file excerpt limited to first {} bytes]\n",
                        self.max_file_bytes
                    ));
                }
                if let Some(content) = &snapshot.content {
                    out.push_str("```text\n");
                    out.push_str(content);
                    if !content.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("```\n");
                } else {
                    out.push_str("```text\n");
                    out.push_str(&format!(
                        "[context: file unreadable — {}]\n",
                        snapshot.path.display()
                    ));
                    out.push_str("```\n");
                }
            }
        }
    }
}

fn resolve_include_git_context(default_enabled: bool) -> bool {
    match std::env::var("VEX_CONTEXT_INCLUDE_GIT") {
        Ok(value) if !value.trim().is_empty() => match parse_bool_flag(value.clone()) {
            Some(enabled) => enabled,
            None => {
                eprintln!(
                    "[context] invalid VEX_CONTEXT_INCLUDE_GIT={value:?}; using default {default_enabled}"
                );
                default_enabled
            }
        },
        _ => default_enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::{AssembledContext, ContextAssembler, FileRollup, extract_candidate_paths};
    use crate::tools::ToolOperator;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    #[tokio::test]
    async fn test_context_assembler_includes_named_file_rollup() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let file_path = workspace.path().join("known-file.txt");
        fs::write(&file_path, "hello from snapshot").expect("write");

        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default();
        let ctx = assembler
            .assemble("please inspect known-file.txt", &operator)
            .expect("assemble failed");

        assert!(
            ctx.file_rollups
                .iter()
                .any(|snapshot| snapshot.path.as_path() == Path::new("known-file.txt")),
            "expected named file to be included in snapshots"
        );
    }

    #[tokio::test]
    async fn test_context_assembler_keeps_unreadable_named_file_rollup() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default();
        let ctx = assembler
            .assemble("inspect missing-file.txt", &operator)
            .expect("assemble failed");
        let rendered = assembler.render(&ctx);

        let snapshot = ctx
            .file_rollups
            .iter()
            .find(|snapshot| snapshot.path.as_path() == Path::new("missing-file.txt"))
            .expect("missing named file rollup");
        assert!(snapshot.content.is_none());
        assert!(rendered.contains("[context: file unreadable — missing-file.txt]"));
    }

    #[tokio::test]
    async fn test_context_assembler_named_paths_not_capped_by_max_related() {
        let workspace = tempfile::tempdir().expect("tempdir");
        for i in 0..5 {
            fs::write(
                workspace.path().join(format!("file{i}.txt")),
                format!("content {i}"),
            )
            .expect("write");
        }

        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler {
            max_related: 2,
            ..ContextAssembler::default()
        };
        let instruction = "inspect file0.txt file1.txt file2.txt file3.txt file4.txt";
        let ctx = assembler
            .assemble(instruction, &operator)
            .expect("assemble failed");

        assert_eq!(
            ctx.file_rollups.len(),
            5,
            "all five named files must be rolled-up regardless of max_related"
        );
    }

    #[test]
    fn test_render_shared_prefix_omits_git_sections() {
        let assembler = ContextAssembler::default();
        let ctx = AssembledContext {
            file_rollups: vec![FileRollup {
                path: PathBuf::from("src/lib.rs"),
                content: Some("pub fn run() {}\n".to_string()),
                content_limited: false,
            }],
            git_status_summary: Some(" M src/lib.rs\n".to_string()),
            recent_diff: Some("diff --git a/src/lib.rs b/src/lib.rs\n".to_string()),
            has_staged_changes: true,
            has_working_tree_changes: true,
            git_dir: Some(PathBuf::from(".git")),
            committer_name: Some("tester".to_string()),
            staged_paths: vec![PathBuf::from("src/lib.rs")],
            related_paths: vec![PathBuf::from("src/runtime/context.rs")],
            cache_hits: 0,
            cache_misses: 1,
        };

        let rendered = assembler.render_shared_prefix(&ctx);

        assert!(rendered.contains("## Shared context prefix"));
        assert!(rendered.contains("src/lib.rs"));
        assert!(rendered.contains("src/runtime/context.rs"));
        assert!(
            !rendered.contains("### Git status"),
            "shared prefix must exclude mutable git status"
        );
        assert!(
            !rendered.contains("### Recent diff"),
            "shared prefix must exclude mutable git diff data"
        );
    }

    #[tokio::test]
    async fn test_context_assembler_reuses_cached_rollups_between_calls() {
        let _lock = crate::runtime::context_cache::lock_context_cache_for_tests();
        crate::runtime::context_cache::reset_context_cache_for_tests();
        let workspace = tempfile::tempdir().expect("tempdir");
        fs::write(workspace.path().join("note.txt"), "cache me\n").expect("write note");

        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default();
        let first = assembler
            .assemble("inspect note.txt", &operator)
            .expect("first assemble failed");
        let second = assembler
            .assemble("inspect note.txt", &operator)
            .expect("second assemble failed");

        assert_eq!(
            first.cache_hits, 0,
            "first assemble must cold-read the file"
        );
        assert_eq!(
            first.cache_misses, 1,
            "first assemble must record one cache miss"
        );
        assert!(
            second.cache_hits >= 1,
            "second assemble should reuse the in-memory rollup cache"
        );
        assert_eq!(
            second.cache_misses, 0,
            "second assemble should not reread the unchanged file from disk"
        );
    }

    #[tokio::test]
    async fn test_context_assembler_default_skips_git_context() {
        let _lock = crate::test_support::ENV_LOCK.lock().await;
        let workspace = tempfile::tempdir().expect("tempdir");
        init_git_repo(workspace.path());
        fs::write(workspace.path().join("note.txt"), "note\n").expect("write note");
        crate::test_support::test_remove_var(&_lock, "VEX_CONTEXT_INCLUDE_GIT");

        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default();
        let ctx = assembler
            .assemble("inspect note.txt", &operator)
            .expect("assemble failed");

        assert!(ctx.git_status_summary.is_none());
        assert!(ctx.recent_diff.is_none());
    }

    #[tokio::test]
    async fn test_context_assembler_infers_related_paths_from_rust_use_lines() {
        let workspace = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(workspace.path().join("src/runtime")).expect("mkdir");
        fs::write(
            workspace.path().join("src/main.rs"),
            "use crate::runtime::helper;\nfn main() {}\n",
        )
        .expect("write main");
        fs::write(
            workspace.path().join("src/runtime/helper.rs"),
            "pub fn run() {}\n",
        )
        .expect("write helper");

        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default();
        let ctx = assembler
            .assemble("inspect src/main.rs", &operator)
            .expect("assemble failed");

        assert!(
            ctx.related_paths
                .iter()
                .any(|path| path.as_path() == Path::new("src/runtime/helper.rs")),
            "expected inferred related path in context"
        );
        assert!(
            ctx.file_rollups
                .iter()
                .any(|snapshot| snapshot.path.as_path() == Path::new("src/runtime/helper.rs")),
            "expected inferred related file rollup"
        );
    }

    #[test]
    fn expanded_scan_raises_related_path_cap() {
        let assembler = ContextAssembler::default().with_expanded_scan(true);
        assert!(assembler.max_related > ContextAssembler::default().max_related);
    }

    #[tokio::test]
    async fn test_context_assembler_non_git_repo_returns_none_diff() {
        let _lock = crate::test_support::ENV_LOCK.lock().await;
        let workspace = tempfile::tempdir().expect("tempdir");
        fs::write(workspace.path().join("note.txt"), "note").expect("write");

        let ceiling = workspace.path().to_string_lossy().to_string();
        crate::test_support::test_set_var(&_lock, "GIT_CEILING_DIRECTORIES", &ceiling);

        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default().with_git_context(true);
        let ctx = assembler
            .assemble("read note.txt", &operator)
            .expect("assemble failed");

        crate::test_support::test_remove_var(&_lock, "GIT_CEILING_DIRECTORIES");

        assert!(ctx.git_status_summary.is_none());
        assert!(ctx.recent_diff.is_none());
    }

    #[tokio::test]
    async fn test_context_assembler_git_timeout_returns_none_with_annotation() {
        let _lock = crate::test_support::ENV_LOCK.lock().await;
        let workspace = tempfile::tempdir().expect("tempdir");
        init_git_repo(workspace.path());

        let file_path = workspace.path().join("slow.txt");
        let original = "a line that will change\n".repeat(80_000);
        fs::write(&file_path, original).expect("write original");
        run_git(workspace.path(), &["add", "."]);
        run_git(
            workspace.path(),
            &[
                "-c",
                "user.name=vex-test",
                "-c",
                "user.email=vex-test@example.com",
                "commit",
                "-m",
                "init",
            ],
        );
        let changed = "different line to force large diff\n".repeat(80_000);
        fs::write(&file_path, changed).expect("write changed");

        crate::test_support::test_set_var(&_lock, "VEX_CONTEXT_GIT_TIMEOUT_MS", "1");
        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default().with_git_context(true);
        let ctx = assembler
            .assemble("inspect slow.txt", &operator)
            .expect("assemble failed");
        let rendered = assembler.render(&ctx);
        crate::test_support::test_remove_var(&_lock, "VEX_CONTEXT_GIT_TIMEOUT_MS");

        assert!(ctx.recent_diff.is_none());
        assert!(
            rendered.contains("timed out"),
            "render output must include timeout annotation: {rendered}"
        );
        assert!(
            rendered.contains("git status timed out"),
            "render output must include status timeout annotation: {rendered}"
        );
    }

    #[tokio::test]
    async fn test_context_assembler_large_diff_does_not_timeout() {
        let _lock = crate::test_support::ENV_LOCK.lock().await;
        let workspace = tempfile::tempdir().expect("tempdir");
        init_git_repo(workspace.path());

        let file_path = workspace.path().join("large-diff.txt");
        let original = "before content line for large diff\n".repeat(4_000);
        fs::write(&file_path, original).expect("write original");
        run_git(workspace.path(), &["add", "."]);
        run_git(
            workspace.path(),
            &[
                "-c",
                "user.name=vex-test",
                "-c",
                "user.email=vex-test@example.com",
                "commit",
                "-m",
                "init",
            ],
        );
        let changed = "after content line for large diff output expansion\n".repeat(4_000);
        assert!(changed.len() > 64_000, "test fixture must exceed pipe size");
        fs::write(&file_path, changed).expect("write changed");

        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default().with_git_context(true);
        let ctx = assembler
            .assemble("inspect large-diff.txt", &operator)
            .expect("assemble failed");

        assert!(
            ctx.recent_diff.is_some(),
            "expected recent diff to be captured for large output"
        );
    }

    #[test]
    fn test_extract_candidate_paths_rejects_version_like_tokens() {
        let paths = extract_candidate_paths("review 0.1.2 Cargo.toml src/lib.rs");
        assert!(
            paths.iter().any(|path| path == "Cargo.toml"),
            "expected valid file token to be kept"
        );
        assert!(
            paths.iter().any(|path| path == "src/lib.rs"),
            "expected path token to be kept"
        );
        assert!(
            paths.iter().all(|path| path != "0.1.2"),
            "version-like token should be filtered out"
        );
    }

    fn init_git_repo(root: &Path) {
        run_git(root, &["init"]);
    }

    fn run_git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .expect("git command failed to start");
        assert!(
            output.status.success(),
            "git command failed: args={args:?}, stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
