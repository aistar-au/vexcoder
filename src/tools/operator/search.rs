use aho_corasick::{AhoCorasick, AhoCorasickBuilder};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::{non_empty_trimmed, path_to_repo_relative_string, SearchMatch, ToolOperator};
use crate::tools::search::parallel_search_files;
use crate::tools::workspace_explore::compile_workspace_glob;

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
        let glob_matcher = match glob_pattern {
            Some(pattern) => match compile_workspace_glob(pattern) {
                Some(matcher) => Some(matcher),
                None => return Err(anyhow::anyhow!("invalid path_glob pattern: {pattern:?}")),
            },
            None => None,
        };

        // Collect candidate paths, applying the optional glob filter.
        let mut candidates: Vec<PathBuf> = Vec::new();
        for path in self.walk_workspace_files(&self.working_dir)? {
            if let Some(matcher) = glob_matcher.as_ref() {
                let relative = path
                    .strip_prefix(&self.working_dir)
                    .unwrap_or_else(|_| Path::new(""));
                let candidate = path_to_repo_relative_string(relative);
                if !matcher.matches(&candidate) {
                    continue;
                }
            }
            candidates.push(path);
        }

        // Escape the literal query to a regex pattern so the grep-searcher /
        // memmap2 backend handles the heavy I/O while preserving literal
        // matching semantics.
        let escaped = regex_lite::escape(query);
        let case_sensitive = query.chars().any(char::is_uppercase);
        let pattern = if case_sensitive {
            escaped
        } else {
            format!("(?i){escaped}")
        };

        let raw = parallel_search_files(&candidates, &pattern);

        let mut matches: Vec<SearchMatch> = raw
            .into_iter()
            .map(|(file_str, line_number, line_text)| SearchMatch {
                file: PathBuf::from(file_str),
                line_number: line_number as usize,
                line_text,
            })
            .collect();

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
        let Some(matcher) = compile_workspace_glob(pattern) else {
            return Err(anyhow::anyhow!("invalid name_glob pattern: {pattern:?}"));
        };

        let mut results = Vec::new();
        for path in self.walk_workspace_files(&self.working_dir)? {
            let relative = path
                .strip_prefix(&self.working_dir)
                .unwrap_or_else(|_| Path::new(""));
            let candidate = path_to_repo_relative_string(relative);
            if matcher.matches(&candidate) {
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
