use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use super::{non_empty_trimmed, path_to_repo_relative_string, SearchMatch, ToolOperator};

impl ToolOperator {
    pub fn search_files(
        &self,
        query: &str,
        path: Option<&str>,
        max_results: usize,
    ) -> Result<String> {
        let query =
            non_empty_trimmed(query).context("search_files requires a non-empty 'query' field")?;
        let root = self.resolve_optional_path(path)?;
        let max_results = max_results.clamp(1, 200);
        self.search_literal(query, &root, max_results)
    }

    pub(super) fn search_literal(
        &self,
        query: &str,
        root: &Path,
        max_results: usize,
    ) -> Result<String> {
        let mut results = Vec::new();
        let (matcher, unicode_case_folded_query) = build_line_matcher(query)?;
        for path in self.walk_workspace_files(root)? {
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };

            for (index, line) in content.lines().enumerate() {
                if line_matches_literal_query(line, &matcher, &unicode_case_folded_query) {
                    results.push(format!(
                        "{}:{}:{}",
                        self.to_workspace_relative_display(&path),
                        index + 1,
                        line
                    ));
                    if results.len() >= max_results {
                        break;
                    }
                }
            }
            if results.len() >= max_results {
                break;
            }
        }

        if results.is_empty() {
            Ok("No matches found.".to_string())
        } else {
            Ok(results.join("\n"))
        }
    }

    pub fn search_content(&self, query: &str, path_glob: Option<&str>) -> Result<Vec<SearchMatch>> {
        let query = non_empty_trimmed(query)
            .context("search_content requires a non-empty 'query' field")?;
        let glob_pattern = path_glob.and_then(non_empty_trimmed);

        let mut matches = Vec::new();
        let (matcher, unicode_case_folded_query) = build_line_matcher(query)?;

        for path in self.walk_workspace_files(&self.working_dir)? {
            let relative = path
                .strip_prefix(&self.working_dir)
                .unwrap_or_else(|_| Path::new(""));
            if let Some(pattern) = glob_pattern {
                let candidate = path_to_repo_relative_string(relative);
                if !glob_matches(pattern, &candidate) {
                    continue;
                }
            }

            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };

            for (index, line) in content.lines().enumerate() {
                if line_matches_literal_query(line, &matcher, &unicode_case_folded_query) {
                    matches.push(SearchMatch {
                        file: path.clone(),
                        line_number: index + 1,
                        line_text: line.to_string(),
                    });
                }
            }
        }

        matches.sort_by(|left, right| {
            left.file
                .cmp(&right.file)
                .then_with(|| left.line_number.cmp(&right.line_number))
        });

        Ok(matches)
    }

    pub fn find_files(&self, name_glob: &str) -> Result<Vec<std::path::PathBuf>> {
        let pattern = non_empty_trimmed(name_glob)
            .context("find_files requires a non-empty 'name_glob' field")?;

        let mut results = Vec::new();
        for path in self.walk_workspace_files(&self.working_dir)? {
            let relative = path
                .strip_prefix(&self.working_dir)
                .unwrap_or_else(|_| Path::new(""));
            let candidate = path_to_repo_relative_string(relative);
            if glob_matches(pattern, &candidate) {
                results.push(path);
            }
        }

        results.sort();
        Ok(results)
    }
}

fn build_line_matcher(query: &str) -> Result<(AhoCorasick, Option<String>)> {
    let case_sensitive = query.chars().any(char::is_uppercase);
    let matcher = AhoCorasickBuilder::new()
        .ascii_case_insensitive(!case_sensitive)
        .build([query])
        .context("Failed to build literal search matcher")?;
    let unicode_case_folded_query = if !case_sensitive && !query.is_ascii() {
        Some(query.to_lowercase())
    } else {
        None
    };
    Ok((matcher, unicode_case_folded_query))
}

fn line_matches_literal_query(
    line: &str,
    matcher: &AhoCorasick,
    unicode_case_folded_query: &Option<String>,
) -> bool {
    if let Some(case_folded_query) = unicode_case_folded_query {
        line.to_lowercase().contains(case_folded_query)
    } else {
        matcher.is_match(line)
    }
}

pub(super) fn glob_matches(pattern: &str, candidate: &str) -> bool {
    wildcard_match(pattern.as_bytes(), candidate.as_bytes())
}

fn wildcard_match(pattern: &[u8], text: &[u8]) -> bool {
    let mut pattern_idx = 0usize;
    let mut text_idx = 0usize;
    let mut star_idx: Option<usize> = None;
    let mut retry_text_idx = 0usize;

    while text_idx < text.len() {
        if pattern_idx < pattern.len()
            && (pattern[pattern_idx] == b'?' || pattern[pattern_idx] == text[text_idx])
        {
            pattern_idx += 1;
            text_idx += 1;
        } else if pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
            star_idx = Some(pattern_idx);
            pattern_idx += 1;
            retry_text_idx = text_idx;
        } else if let Some(star) = star_idx {
            pattern_idx = star + 1;
            retry_text_idx += 1;
            text_idx = retry_text_idx;
        } else {
            return false;
        }
    }

    while pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
        pattern_idx += 1;
    }

    pattern_idx == pattern.len()
}
