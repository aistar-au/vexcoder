//! Structured parsing of git command output using `regex-lite`.
//!
//! Design boundary: `regex-lite` handles ASCII-only internal string surgery
//! on git's machine-readable output formats.  This is distinct from
//! `aho-corasick` (multi-pattern literal matching in file content),
//! `tree-sitter` (structural AST indexing), and `globset`/`ignore`
//! (filesystem traversal).  None of these crates overlap in purpose.

use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single entry from `git status --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// Index (staging area) status character.
    pub index: char,
    /// Worktree status character.
    pub worktree: char,
    /// File path (includes ` -> new` suffix for renames).
    pub path: String,
}

/// Parsed result from `git status --porcelain`.
#[derive(Debug, Clone, Default)]
pub struct ParsedGitStatus {
    pub entries: Vec<StatusEntry>,
}

/// Outcome classification for a single line in `git apply` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied { path: String },
    Conflict { path: String },
    Rejected { path: String },
    AlreadyApplied { path: String },
    DoesNotApply { message: String },
    BinaryPatch { path: String },
    Failed { message: String },
}

/// Parsed result from `git apply` combined stdout+stderr.
#[derive(Debug, Clone, Default)]
pub struct ParsedGitApply {
    pub outcomes: Vec<ApplyOutcome>,
    pub has_errors: bool,
}

/// A single file entry from `git diff --stat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffStatEntry {
    pub path: String,
    pub changes: u32,
}

/// Parsed `git diff --stat` output with per-file entries and summary.
#[derive(Debug, Clone, Default)]
pub struct ParsedDiffStat {
    pub entries: Vec<DiffStatEntry>,
    pub total_files: u32,
    pub total_insertions: u32,
    pub total_deletions: u32,
}

// ---------------------------------------------------------------------------
// Regex compilation helpers — compile once via OnceLock
// ---------------------------------------------------------------------------

fn re_status_line() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"^(.)(.) (.+)$").unwrap())
}

fn re_diff_stat_line() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"^\s*(.+?)\s*\|\s*(\d+)").unwrap())
}

fn re_diff_stat_summary() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(
            r"(\d+) files? changed(?:, (\d+) insertions?\(\+\))?(?:, (\d+) deletions?\(-\))?",
        )
        .unwrap()
    })
}

fn re_apply_applied() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"(?i)^applying:?\s+(.+)$").unwrap())
}

fn re_apply_conflict() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"(?i)^conflict\b.*?:\s*(.+)$").unwrap())
}

fn re_apply_rejected() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(r"(?i)(?:rejected|error:.*rejected).*?(?:for|to)\s+(.+)$")
            .unwrap()
    })
}

fn re_apply_already() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| regex_lite::Regex::new(r"(?i)already applied.*?[:\s]+(.+)$").unwrap())
}

fn re_apply_does_not_apply() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(r"(?i)(?:does not apply|patch failed):\s*(.+)$").unwrap()
    })
}

fn re_apply_binary() -> &'static regex_lite::Regex {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex_lite::Regex::new(r"(?i)(?:cannot apply binary|binary patch).*?(?:to|:)\s+'?([^'\s]+)")
            .unwrap()
    })
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

/// Parse `git status --porcelain` output into structured entries.
pub fn parse_git_status(output: &str) -> ParsedGitStatus {
    let re = re_status_line();
    let entries = output
        .lines()
        .filter_map(|line| {
            let caps = re.captures(line)?;
            Some(StatusEntry {
                index: caps[1].chars().next().unwrap_or(' '),
                worktree: caps[2].chars().next().unwrap_or(' '),
                path: caps[3].to_string(),
            })
        })
        .collect();
    ParsedGitStatus { entries }
}

/// Parse `git diff --stat` output into structured entries and a summary.
pub fn parse_diff_stat(output: &str) -> ParsedDiffStat {
    let re_line = re_diff_stat_line();
    let re_summary = re_diff_stat_summary();
    let mut result = ParsedDiffStat::default();

    for line in output.lines() {
        if let Some(caps) = re_summary.captures(line) {
            result.total_files = caps
                .get(1)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            result.total_insertions = caps
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            result.total_deletions = caps
                .get(3)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            continue;
        }
        if let Some(caps) = re_line.captures(line) {
            let path = caps[1].trim().to_string();
            let changes: u32 = caps[2].parse().unwrap_or(0);
            result.entries.push(DiffStatEntry { path, changes });
        }
    }
    result
}

/// Parse `git apply` combined stdout+stderr into structured outcomes.
pub fn parse_git_apply(output: &str) -> ParsedGitApply {
    let mut outcomes = Vec::new();
    let mut has_errors = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(caps) = re_apply_conflict().captures(trimmed) {
            outcomes.push(ApplyOutcome::Conflict {
                path: caps[1].to_string(),
            });
            has_errors = true;
        } else if let Some(caps) = re_apply_rejected().captures(trimmed) {
            outcomes.push(ApplyOutcome::Rejected {
                path: caps[1].to_string(),
            });
            has_errors = true;
        } else if let Some(caps) = re_apply_already().captures(trimmed) {
            outcomes.push(ApplyOutcome::AlreadyApplied {
                path: caps[1].to_string(),
            });
        } else if let Some(caps) = re_apply_does_not_apply().captures(trimmed) {
            outcomes.push(ApplyOutcome::DoesNotApply {
                message: caps[1].to_string(),
            });
            has_errors = true;
        } else if let Some(caps) = re_apply_binary().captures(trimmed) {
            outcomes.push(ApplyOutcome::BinaryPatch {
                path: caps[1].to_string(),
            });
        } else if let Some(caps) = re_apply_applied().captures(trimmed) {
            outcomes.push(ApplyOutcome::Applied {
                path: caps[1].to_string(),
            });
        } else if trimmed.starts_with("error:") || trimmed.starts_with("fatal:") {
            outcomes.push(ApplyOutcome::Failed {
                message: trimmed.to_string(),
            });
            has_errors = true;
        }
    }

    ParsedGitApply {
        outcomes,
        has_errors,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_git_status_basic() {
        let output = " M src/main.rs\n?? new_file.txt\nA  staged.rs\nMM both.rs";
        let parsed = parse_git_status(output);
        assert_eq!(parsed.entries.len(), 4);

        assert_eq!(parsed.entries[0].index, ' ');
        assert_eq!(parsed.entries[0].worktree, 'M');
        assert_eq!(parsed.entries[0].path, "src/main.rs");

        assert_eq!(parsed.entries[1].index, '?');
        assert_eq!(parsed.entries[1].worktree, '?');
        assert_eq!(parsed.entries[1].path, "new_file.txt");

        assert_eq!(parsed.entries[2].index, 'A');
        assert_eq!(parsed.entries[2].worktree, ' ');
        assert_eq!(parsed.entries[2].path, "staged.rs");

        assert_eq!(parsed.entries[3].index, 'M');
        assert_eq!(parsed.entries[3].worktree, 'M');
        assert_eq!(parsed.entries[3].path, "both.rs");
    }

    #[test]
    fn test_parse_git_status_rename() {
        let output = "R  old.rs -> new.rs";
        let parsed = parse_git_status(output);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].index, 'R');
        assert_eq!(parsed.entries[0].path, "old.rs -> new.rs");
    }

    #[test]
    fn test_parse_git_status_empty() {
        let parsed = parse_git_status("");
        assert!(parsed.entries.is_empty());
    }

    #[test]
    fn test_parse_diff_stat_typical() {
        let output = " src/main.rs   | 10\n src/lib.rs    |  3\n 2 files changed, 7 insertions(+), 6 deletions(-)";
        let parsed = parse_diff_stat(output);
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].path, "src/main.rs");
        assert_eq!(parsed.entries[0].changes, 10);
        assert_eq!(parsed.entries[1].path, "src/lib.rs");
        assert_eq!(parsed.entries[1].changes, 3);
        assert_eq!(parsed.total_files, 2);
        assert_eq!(parsed.total_insertions, 7);
        assert_eq!(parsed.total_deletions, 6);
    }

    #[test]
    fn test_parse_diff_stat_insertions_only() {
        let output = " 1 file changed, 5 insertions(+)";
        let parsed = parse_diff_stat(output);
        assert_eq!(parsed.total_files, 1);
        assert_eq!(parsed.total_insertions, 5);
        assert_eq!(parsed.total_deletions, 0);
    }

    #[test]
    fn test_parse_diff_stat_deletions_only() {
        let output = " 1 file changed, 3 deletions(-)";
        let parsed = parse_diff_stat(output);
        assert_eq!(parsed.total_files, 1);
        assert_eq!(parsed.total_insertions, 0);
        assert_eq!(parsed.total_deletions, 3);
    }

    #[test]
    fn test_parse_git_apply_clean() {
        let output = "Applying: fix typo in README\nApplying: update config";
        let parsed = parse_git_apply(output);
        assert_eq!(parsed.outcomes.len(), 2);
        assert!(!parsed.has_errors);
        assert_eq!(
            parsed.outcomes[0],
            ApplyOutcome::Applied {
                path: "fix typo in README".to_string()
            }
        );
    }

    #[test]
    fn test_parse_git_apply_conflict() {
        let output = "CONFLICT (content): Merge conflict in src/main.rs";
        let parsed = parse_git_apply(output);
        assert_eq!(parsed.outcomes.len(), 1);
        assert!(parsed.has_errors);
        assert_eq!(
            parsed.outcomes[0],
            ApplyOutcome::Conflict {
                path: "Merge conflict in src/main.rs".to_string()
            }
        );
    }

    #[test]
    fn test_parse_git_apply_rejected() {
        let output = "error: patch rejected hunk for src/lib.rs";
        let parsed = parse_git_apply(output);
        assert_eq!(parsed.outcomes.len(), 1);
        assert!(parsed.has_errors);
        assert_eq!(
            parsed.outcomes[0],
            ApplyOutcome::Rejected {
                path: "src/lib.rs".to_string()
            }
        );
    }

    #[test]
    fn test_parse_git_apply_does_not_apply() {
        let output = "error: patch does not apply: src/old.rs";
        let parsed = parse_git_apply(output);
        assert_eq!(parsed.outcomes.len(), 1);
        assert!(parsed.has_errors);
        assert_eq!(
            parsed.outcomes[0],
            ApplyOutcome::DoesNotApply {
                message: "src/old.rs".to_string()
            }
        );
    }

    #[test]
    fn test_parse_git_apply_binary() {
        let output = "Cannot apply binary patch to image.png";
        let parsed = parse_git_apply(output);
        assert_eq!(parsed.outcomes.len(), 1);
        assert!(!parsed.has_errors);
        assert_eq!(
            parsed.outcomes[0],
            ApplyOutcome::BinaryPatch {
                path: "image.png".to_string()
            }
        );
    }

    #[test]
    fn test_parse_git_apply_fatal() {
        let output = "fatal: unable to read tree object";
        let parsed = parse_git_apply(output);
        assert_eq!(parsed.outcomes.len(), 1);
        assert!(parsed.has_errors);
        assert_eq!(
            parsed.outcomes[0],
            ApplyOutcome::Failed {
                message: "fatal: unable to read tree object".to_string()
            }
        );
    }

    #[test]
    fn test_parse_git_apply_mixed() {
        let output =
            "Applying: first patch\nCONFLICT (content): Merge conflict in src/main.rs\nerror: patch rejected hunk for src/lib.rs";
        let parsed = parse_git_apply(output);
        assert_eq!(parsed.outcomes.len(), 3);
        assert!(parsed.has_errors);
        assert!(matches!(parsed.outcomes[0], ApplyOutcome::Applied { .. }));
        assert!(matches!(parsed.outcomes[1], ApplyOutcome::Conflict { .. }));
        assert!(matches!(parsed.outcomes[2], ApplyOutcome::Rejected { .. }));
    }

    #[test]
    fn test_parse_git_apply_empty() {
        let parsed = parse_git_apply("");
        assert!(parsed.outcomes.is_empty());
        assert!(!parsed.has_errors);
    }
}
