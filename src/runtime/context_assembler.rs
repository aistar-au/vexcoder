use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::runtime::context_cache::{read_cached_file, CachedFileRead};
use crate::runtime::git_snapshot::{collect_git_snapshot, resolve_git_timeout_ms};
use crate::runtime::text_util::truncate_head_bytes;
use crate::tools::ToolOperator;
use crate::util::parse_bool_flag;

const DEFAULT_MAX_FILE_BYTES: usize = 32_768;
const DEFAULT_MAX_DIFF_LINES: usize = 200;
const DEFAULT_MAX_RELATED: usize = 3;
const DEFAULT_GIT_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_INCLUDE_GIT_CONTEXT: bool = false;
const STANDALONE_PATH_EXTENSIONS: &[&str] = &["rs", "toml", "md", "txt", "json", "sh"];

#[derive(Debug, Clone)]
pub struct AssembledContext {
    pub file_snapshots: Vec<FileSnapshot>,
    pub git_status_summary: Option<String>,
    pub recent_diff: Option<String>,
    pub related_paths: Vec<PathBuf>,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

#[derive(Debug, Clone)]
pub struct FileSnapshot {
    pub path: PathBuf,
    pub content: Option<String>,
    pub truncated: bool,
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
    pub fn with_git_context(mut self, enabled: bool) -> Self {
        self.include_git_context = enabled;
        self
    }

    /// Assemble context for the given instruction.
    pub fn assemble(&self, instruction: &str, operator: &ToolOperator) -> Result<AssembledContext> {
        let timeout_ms = resolve_git_timeout_ms(self.git_timeout_ms);
        let mut file_snapshots = Vec::new();
        let mut related_paths = Vec::new();
        let mut seen_paths = HashSet::new();
        let mut cache_hits = 0;
        let mut cache_misses = 0;

        // All explicitly named paths are captured without a count cap.
        // The max_related cap applies only to *inferred* related paths below.
        for candidate in extract_candidate_paths(instruction).into_iter() {
            let path = PathBuf::from(&candidate);
            if !seen_paths.insert(path.clone()) {
                continue;
            }
            let (snapshot, cache_hit) = snapshot_from_read(
                path,
                read_cached_file(operator, &candidate),
                self.max_file_bytes,
            );
            if cache_hit {
                cache_hits += 1;
            } else if snapshot.content.is_some() {
                cache_misses += 1;
            }
            file_snapshots.push(snapshot);
        }

        let named_snapshot_count = file_snapshots.len();
        for index in 0..named_snapshot_count {
            if related_paths.len() >= self.max_related {
                break;
            }
            let Some(content) = file_snapshots
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
                let (content, truncated) = truncate_head_bytes(&read.content, self.max_file_bytes);
                if read.cache_hit {
                    cache_hits += 1;
                } else {
                    cache_misses += 1;
                }
                file_snapshots.push(FileSnapshot {
                    path: inferred.clone(),
                    content: Some(content),
                    truncated,
                });
                related_paths.push(inferred);
            }
        }

        if !self.include_git_context {
            return Ok(AssembledContext {
                file_snapshots,
                git_status_summary: None,
                recent_diff: None,
                related_paths,
                cache_hits,
                cache_misses,
            });
        }

        let git_snapshot = collect_git_snapshot(
            operator.working_dir().to_path_buf(),
            timeout_ms,
            self.max_diff_lines,
        )?;

        Ok(AssembledContext {
            file_snapshots,
            git_status_summary: git_snapshot.git_status_summary,
            recent_diff: git_snapshot.recent_diff,
            related_paths,
            cache_hits,
            cache_misses,
        })
    }

    /// Render an `AssembledContext` to a markdown string for injection into a turn.
    pub fn render(&self, ctx: &AssembledContext) -> String {
        let mut out = String::new();
        out.push_str("## Context\n");

        if ctx.file_snapshots.is_empty() {
            out.push_str("[context: no file snapshots]\n");
        } else {
            out.push_str("### File snapshots\n");
            for snapshot in &ctx.file_snapshots {
                out.push_str(&format!("- {}\n", snapshot.path.display()));
                if snapshot.truncated {
                    out.push_str(&format!(
                        "  [context: file truncated to first {} bytes]\n",
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

fn extract_candidate_paths(instruction: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for token in instruction.split_whitespace() {
        let candidate = token.trim_matches(|c: char| {
            matches!(
                c,
                '"' | '\'' | '`' | ',' | '.' | ';' | ':' | '(' | ')' | '[' | ']' | '{' | '}'
            )
        });
        if candidate.is_empty() {
            continue;
        }
        if candidate.starts_with('/') || candidate.starts_with('-') {
            continue;
        }
        if !(candidate.contains('/') || candidate.contains('.')) {
            continue;
        }

        let normalized = candidate.trim_start_matches("./").to_string();
        if normalized.is_empty() {
            continue;
        }
        if normalized
            .chars()
            .next()
            .is_some_and(|value| value.is_ascii_digit())
        {
            continue;
        }
        if !normalized.contains('/') {
            let Some(extension) = Path::new(&normalized)
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase())
            else {
                continue;
            };
            if !STANDALONE_PATH_EXTENSIONS.contains(&extension.as_str()) {
                continue;
            }
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }

    out
}

fn snapshot_from_read(
    path: PathBuf,
    result: Result<CachedFileRead>,
    max_file_bytes: usize,
) -> (FileSnapshot, bool) {
    match result {
        Ok(read) => {
            let (content, truncated) = truncate_head_bytes(&read.content, max_file_bytes);
            (
                FileSnapshot {
                    path,
                    content: Some(content),
                    truncated,
                },
                read.cache_hit,
            )
        }
        Err(_) => (
            FileSnapshot {
                path,
                content: None,
                truncated: false,
            },
            false,
        ),
    }
}

fn infer_related_path_candidates(content: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(path) = infer_rust_use_path(trimmed) {
            if seen.insert(path.clone()) {
                out.push(path);
            }
        }

        if let Some(path) = infer_python_import_path(trimmed) {
            if seen.insert(path.clone()) {
                out.push(path);
            }
        }

        if let Some(path) = infer_js_import_path(trimmed) {
            if seen.insert(path.clone()) {
                out.push(path);
            }
        }
    }

    out
}

fn infer_rust_use_path(line: &str) -> Option<PathBuf> {
    let value = line
        .strip_prefix("use ")
        .or_else(|| line.strip_prefix("pub use "))?;
    let mut module = value
        .split("//")
        .next()?
        .trim()
        .trim_end_matches(';')
        .trim();
    if module.is_empty() {
        return None;
    }
    if let Some((prefix, _)) = module.split_once(" as ") {
        module = prefix.trim();
    }
    if let Some((prefix, _)) = module.split_once('{') {
        module = prefix.trim();
    }
    module = module.trim_end_matches(':').trim_end_matches(':').trim();

    let relative = module
        .strip_prefix("crate::")
        .or_else(|| module.strip_prefix("super::"))
        .or_else(|| module.strip_prefix("self::"))?;
    if relative.is_empty() {
        return None;
    }
    let path = relative.replace("::", "/");
    Some(PathBuf::from("src").join(format!("{path}.rs")))
}

fn infer_python_import_path(line: &str) -> Option<PathBuf> {
    let module = if let Some(value) = line.strip_prefix("from ") {
        let (module, _) = value.split_once(" import ")?;
        module.trim()
    } else if let Some(value) = line.strip_prefix("import ") {
        if value.contains(" from ") || value.contains('"') || value.contains('\'') {
            return None;
        }
        value
            .split(',')
            .next()
            .and_then(|entry| entry.split_whitespace().next())?
            .trim()
    } else {
        return None;
    };

    if module.is_empty() || module.starts_with('.') {
        return None;
    }
    let path = module.replace('.', "/");
    Some(PathBuf::from(format!("{path}.py")))
}

fn infer_js_import_path(line: &str) -> Option<PathBuf> {
    if !line.starts_with("import ") && !line.starts_with("export ") {
        return None;
    }
    let specifier = extract_quoted_specifier(line)?;
    if specifier.is_empty() || specifier.starts_with('/') || specifier.starts_with("../") {
        return None;
    }

    let normalized = specifier.trim_start_matches("./");
    if normalized.is_empty() {
        return None;
    }
    if Path::new(normalized).extension().is_some() {
        return Some(PathBuf::from(normalized));
    }
    Some(PathBuf::from(format!("{normalized}.js")))
}

fn extract_quoted_specifier(line: &str) -> Option<&str> {
    for quote in ['"', '\''] {
        if let Some(start) = line.find(quote) {
            let tail = &line[start + 1..];
            if let Some(end) = tail.find(quote) {
                return Some(&tail[..end]);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::ContextAssembler;
    use crate::tools::ToolOperator;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    #[tokio::test]
    async fn test_context_assembler_includes_named_file_snapshot() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let file_path = workspace.path().join("known-file.txt");
        fs::write(&file_path, "hello from snapshot").expect("write");

        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default();
        let ctx = assembler
            .assemble("please inspect known-file.txt", &operator)
            .expect("assemble failed");

        assert!(
            ctx.file_snapshots
                .iter()
                .any(|snapshot| snapshot.path.as_path() == Path::new("known-file.txt")),
            "expected named file to be included in snapshots"
        );
    }

    #[tokio::test]
    async fn test_context_assembler_keeps_unreadable_named_file_snapshot() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default();
        let ctx = assembler
            .assemble("inspect missing-file.txt", &operator)
            .expect("assemble failed");
        let rendered = assembler.render(&ctx);

        let snapshot = ctx
            .file_snapshots
            .iter()
            .find(|snapshot| snapshot.path.as_path() == Path::new("missing-file.txt"))
            .expect("missing named file snapshot");
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
            ctx.file_snapshots.len(),
            5,
            "all five named files must be snapshotted regardless of max_related"
        );
    }

    #[tokio::test]
    async fn test_context_assembler_reuses_cached_snapshots_between_calls() {
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
            "second assemble should reuse the in-memory snapshot cache"
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
        std::env::remove_var("VEX_CONTEXT_INCLUDE_GIT");

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
            ctx.file_snapshots
                .iter()
                .any(|snapshot| snapshot.path.as_path() == Path::new("src/runtime/helper.rs")),
            "expected inferred related file snapshot"
        );
    }

    #[tokio::test]
    async fn test_context_assembler_non_git_repo_returns_none_diff() {
        let _lock = crate::test_support::ENV_LOCK.lock().await;
        let workspace = tempfile::tempdir().expect("tempdir");
        fs::write(workspace.path().join("note.txt"), "note").expect("write");

        // On some macOS CI runners TMPDIR resolves to a path inside the Actions
        // work tree, so `git status` in the tempdir succeeds by discovering a
        // parent repo.  Set GIT_CEILING_DIRECTORIES to the workspace itself so
        // git cannot traverse above it; the workspace has no .git, which is the
        // condition we are testing.
        let ceiling = workspace.path().to_string_lossy().to_string();
        std::env::set_var("GIT_CEILING_DIRECTORIES", &ceiling);

        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default().with_git_context(true);
        let ctx = assembler
            .assemble("read note.txt", &operator)
            .expect("assemble failed");

        std::env::remove_var("GIT_CEILING_DIRECTORIES");

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

        std::env::set_var("VEX_CONTEXT_GIT_TIMEOUT_MS", "1");
        let operator = ToolOperator::new(workspace.path().to_path_buf());
        let assembler = ContextAssembler::default().with_git_context(true);
        let ctx = assembler
            .assemble("inspect slow.txt", &operator)
            .expect("assemble failed");
        let rendered = assembler.render(&ctx);
        std::env::remove_var("VEX_CONTEXT_GIT_TIMEOUT_MS");

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
        let paths = super::extract_candidate_paths("review 0.1.2 Cargo.toml src/lib.rs");
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
