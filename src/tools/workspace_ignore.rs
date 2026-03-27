use std::fs;
use std::path::Path;

/// A set of `.gitignore` patterns loaded from the workspace root.
///
/// Parses the workspace-root `.gitignore` file and answers whether a given
/// workspace-relative path should be excluded from file-system walks.
///
/// Implemented in `std` only: no subprocess calls, no regex crate.
#[derive(Default)]
pub struct WorkspaceIgnore {
    patterns: Vec<IgnorePattern>,
}

struct IgnorePattern {
    text: String,
    negated: bool,
    rooted: bool,
}

impl WorkspaceIgnore {
    /// Load `.gitignore` from `workspace_root`.  Returns an empty ignore set
    /// on any read or parse error so the walk always continues safely.
    pub fn load(workspace_root: &Path) -> Self {
        let path = workspace_root.join(".gitignore");
        let Ok(content) = fs::read_to_string(&path) else {
            return Self::default();
        };
        let patterns = content.lines().filter_map(parse_line).collect();
        Self { patterns }
    }

    /// Returns `true` when `relative_path` (forward-slash separated, no
    /// leading slash) should be excluded from a workspace walk.
    pub fn is_ignored(&self, relative_path: &str) -> bool {
        let mut ignored = false;
        for p in &self.patterns {
            if p.matches(relative_path) {
                ignored = !p.negated;
            }
        }
        ignored
    }
}

impl IgnorePattern {
    fn matches(&self, relative_path: &str) -> bool {
        let last = relative_path.rsplit('/').next().unwrap_or(relative_path);
        if self.rooted {
            // Anchored pattern — must match from workspace root.
            gitignore_glob_match(&self.text, relative_path)
        } else if self.text.contains('/') {
            // Pattern with slash — match full path or any suffix.
            gitignore_glob_match(&self.text, relative_path)
                || gitignore_glob_match(&self.text, last)
        } else {
            // Bare name or extension glob.
            // 1. Match the full relative path (e.g. "target" vs "target").
            // 2. Match the last segment (e.g. "*.log" vs "error.log").
            // 3. Match any path component prefix — covers directory patterns
            //    like "target/" which should ignore "target/debug/vex".
            if gitignore_glob_match(&self.text, relative_path)
                || gitignore_glob_match(&self.text, last)
            {
                return true;
            }
            // Walk each component of the path.
            let mut rest = relative_path;
            while let Some(slash) = rest.find('/') {
                let component = &rest[..slash];
                if gitignore_glob_match(&self.text, component) {
                    return true;
                }
                rest = &rest[slash + 1..];
            }
            false
        }
    }
}

fn parse_line(line: &str) -> Option<IgnorePattern> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (negated, line) = match line.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, line),
    };
    // Strip trailing slash (directory-only markers are treated the same for
    // our walk: we skip the entry regardless of whether it is a file or dir).
    let line = line.strip_suffix('/').unwrap_or(line);
    let (rooted, text) = match line.strip_prefix('/') {
        Some(rest) => (true, rest.to_string()),
        None => (false, line.to_string()),
    };
    if text.is_empty() {
        return None;
    }
    Some(IgnorePattern {
        text,
        negated,
        rooted,
    })
}

/// Glob match using `*` (matches any segment chars except `/`), `**`
/// (matches across `/`), `?` (matches one non-`/` char), and bracket
/// character classes such as `[abc]`, `[a-z]`, and `[^x]`.
///
/// Simple linear scan — no backtracking across `**`; sufficient for the
/// patterns found in real `.gitignore` files.
fn gitignore_glob_match(pattern: &str, text: &str) -> bool {
    gitignore_glob(pattern.as_bytes(), text.as_bytes())
}

fn gitignore_glob(mut pat: &[u8], mut txt: &[u8]) -> bool {
    loop {
        match (pat.first(), txt.first()) {
            (None, None) => return true,
            (None, _) => return false,
            (Some(b'*'), _) if pat.get(1) == Some(&b'*') => {
                // `**` — consume separator if present
                let rest_pat = if pat.get(2) == Some(&b'/') {
                    &pat[3..]
                } else {
                    &pat[2..]
                };
                // Try matching rest_pat at every position in txt
                let mut cursor = txt;
                loop {
                    if gitignore_glob(rest_pat, cursor) {
                        return true;
                    }
                    match cursor.first() {
                        None => return false,
                        Some(_) => {
                            cursor = &cursor[1..];
                        }
                    }
                }
            }
            (Some(b'*'), _) => {
                // `*` — matches any run of non-`/` chars
                let rest_pat = &pat[1..];
                let mut cursor = txt;
                loop {
                    if gitignore_glob(rest_pat, cursor) {
                        return true;
                    }
                    match cursor.first() {
                        None | Some(b'/') => return false,
                        Some(_) => {
                            cursor = &cursor[1..];
                        }
                    }
                }
            }
            (Some(b'?'), Some(b'/')) | (Some(b'?'), None) => return false,
            (Some(b'?'), Some(_)) => {
                pat = &pat[1..];
                txt = &txt[1..];
            }
            (Some(b'['), Some(t)) => match match_char_class(pat, *t) {
                Some((true, consumed)) => {
                    pat = &pat[consumed..];
                    txt = &txt[1..];
                }
                Some((false, _)) => return false,
                None if *t == b'[' => {
                    pat = &pat[1..];
                    txt = &txt[1..];
                }
                None => return false,
            },
            (Some(p), Some(t)) if p == t => {
                pat = &pat[1..];
                txt = &txt[1..];
            }
            _ => return false,
        }
    }
}

fn match_char_class(pat: &[u8], ch: u8) -> Option<(bool, usize)> {
    if pat.first() != Some(&b'[') {
        return None;
    }

    let mut idx = 1;
    let negated = matches!(pat.get(idx), Some(b'!') | Some(b'^'));
    if negated {
        idx += 1;
    }

    let mut matched = false;
    let mut saw_entry = false;

    while idx < pat.len() {
        let current = pat[idx];
        if current == b']' && saw_entry {
            let in_class = ch != b'/' && matched;
            let final_match = if negated {
                !in_class && ch != b'/'
            } else {
                in_class
            };
            return Some((final_match, idx + 1));
        }

        saw_entry = true;

        if idx + 2 < pat.len() && pat[idx + 1] == b'-' && pat[idx + 2] != b']' {
            let start = current;
            let end = pat[idx + 2];
            if start <= ch && ch <= end {
                matched = true;
            }
            idx += 3;
        } else {
            if current == ch {
                matched = true;
            }
            idx += 1;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn workspace_with(rules: &str) -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), rules).unwrap();
        dir
    }

    #[test]
    fn test_wildcard_star_extension() {
        // `*` does not cross `/` — bare segment match only.
        assert!(gitignore_glob_match("*.log", "error.log"));
        assert!(!gitignore_glob_match("*.log", "dir/error.log"));
        assert!(!gitignore_glob_match("*.log", "error.rs"));
    }

    #[test]
    fn test_wildcard_double_star() {
        assert!(gitignore_glob_match("**/*.log", "dir/sub/error.log"));
        assert!(gitignore_glob_match("**/*.log", "error.log"));
        assert!(!gitignore_glob_match("**/*.log", "error.rs"));
    }

    #[test]
    fn test_wildcard_question_mark() {
        assert!(gitignore_glob_match("file?.txt", "file1.txt"));
        assert!(!gitignore_glob_match("file?.txt", "file12.txt"));
        assert!(!gitignore_glob_match("file?.txt", "file/.txt"));
    }

    #[test]
    fn test_character_class_literal() {
        assert!(gitignore_glob_match(".session[a]rea", ".sessionarea"));
        assert!(!gitignore_glob_match(".session[a]rea", ".sessionbrea"));
    }

    #[test]
    fn test_character_class_range() {
        assert!(gitignore_glob_match("file[0-9].txt", "file7.txt"));
        assert!(!gitignore_glob_match("file[0-9].txt", "filex.txt"));
    }

    #[test]
    fn test_workspace_ignore_loads_extension_pattern() {
        let dir = workspace_with("*.log\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("error.log"));
        assert!(ign.is_ignored("logs/error.log"));
        assert!(!ign.is_ignored("main.rs"));
    }

    #[test]
    fn test_workspace_ignore_directory_pattern() {
        let dir = workspace_with("target/\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("target"));
        assert!(ign.is_ignored("target/debug/vex"));
    }

    #[test]
    fn test_workspace_ignore_negation() {
        let dir = workspace_with("*.log\n!keep.log\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("discard.log"));
        assert!(!ign.is_ignored("keep.log"));
    }

    #[test]
    fn test_workspace_ignore_comment_and_blank() {
        let dir = workspace_with("# comment\n\n*.tmp\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("scratch.tmp"));
        assert!(!ign.is_ignored("Cargo.toml"));
    }

    #[test]
    fn test_workspace_ignore_no_gitignore_never_ignores() {
        let dir = tempfile::tempdir().unwrap();
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(!ign.is_ignored("src/main.rs"));
        assert!(!ign.is_ignored("anything"));
    }

    #[test]
    fn test_workspace_ignore_rooted_pattern() {
        let dir = workspace_with("/CHANGELOG.md\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored("CHANGELOG.md"));
        assert!(!ign.is_ignored("docs/CHANGELOG.md"));
    }

    #[test]
    fn test_workspace_ignore_character_class_pattern() {
        let dir = workspace_with(".session[a]rea/\n");
        let ign = WorkspaceIgnore::load(dir.path());
        assert!(ign.is_ignored(".sessionarea"));
        assert!(ign.is_ignored(".sessionarea/settings.json"));
        assert!(!ign.is_ignored(".sessionbrea/settings.json"));
    }
}
