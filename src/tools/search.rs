use crate::tools::index::IndexChunk;

const DEFAULT_MAX_RESULTS: usize = 10;

/// A single ranked result from [`codebase_search`].
pub struct SearchResult<'a> {
    pub chunk: &'a IndexChunk,
    pub score: f64,
    /// Source text, potentially trimmed to a fixed number of lines.
    pub snippet: String,
}

/// Search the structural index for `query` and return at most `max_results`
/// results sorted by relevance score (highest first).
pub fn codebase_search<'a>(
    query: &str,
    index: &'a [IndexChunk],
    max_results: usize,
) -> Vec<SearchResult<'a>> {
    let query_lower = query.to_lowercase();
    let words: Vec<&str> = query_lower.split_whitespace().collect();

    let mut results: Vec<SearchResult<'a>> = index
        .iter()
        .filter_map(|chunk| {
            let name_lower = chunk.name.to_lowercase();
            let mut score = 0.0f64;

            // Exact name match
            if name_lower == query_lower {
                score += 100.0;
            }
            // Case-insensitive name contains query
            if name_lower.contains(query_lower.as_str()) {
                score += 50.0;
            }
            // Parent scope contains query
            if let Some(parent) = &chunk.parent_scope {
                if parent.to_lowercase().contains(query_lower.as_str()) {
                    score += 20.0;
                }
            }
            // Content keyword match: count occurrences of each query word in the source
            let source_lower = chunk.source.to_lowercase();
            for word in &words {
                let count = source_lower.matches(*word).count();
                score += (count as f64) * 5.0;
            }

            if score > 0.0 {
                let snippet = trim_snippet(&chunk.source, 10);
                Some(SearchResult {
                    chunk,
                    score,
                    snippet,
                })
            } else {
                None
            }
        })
        .collect();

    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    results.truncate(max_results);
    results
}

/// Return the configured default for max search results.
pub fn default_max_results() -> usize {
    std::env::var("VEX_SEARCH_MAX_RESULTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MAX_RESULTS)
}

fn trim_snippet(source: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if lines.len() <= max_lines {
        source.to_string()
    } else {
        let trimmed = lines[..max_lines].join("\n");
        format!("{trimmed}\n  ...")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::index::{IndexChunk, ItemKind};

    fn make_chunk(name: &str, kind: ItemKind, source: &str) -> IndexChunk {
        IndexChunk {
            path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 3,
            kind,
            name: name.to_string(),
            parent_scope: None,
            source: source.to_string(),
        }
    }

    #[test]
    fn test_exact_name_match_scores_highest() {
        let chunks = vec![
            make_chunk("unrelated", ItemKind::Function, "fn unrelated() {}"),
            make_chunk("build_index", ItemKind::Function, "pub fn build_index() {}"),
        ];
        let results = codebase_search("build_index", &chunks, 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].chunk.name, "build_index");
    }

    #[test]
    fn test_no_results_for_unmatched_query() {
        let chunks = vec![make_chunk("foo", ItemKind::Struct, "struct foo {}")];
        let results = codebase_search("zzznomatch", &chunks, 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_max_results_respected() {
        let chunks: Vec<IndexChunk> = (0..20)
            .map(|i| make_chunk(&format!("item_{i}"), ItemKind::Function, "fn item() {}"))
            .collect();
        let results = codebase_search("item", &chunks, 5);
        assert!(results.len() <= 5);
    }

    #[test]
    fn test_snippet_trimmed_to_max_lines() {
        let long_source = (0..20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let chunks = vec![make_chunk("long_fn", ItemKind::Function, &long_source)];
        let results = codebase_search("long", &chunks, 10);
        assert!(!results.is_empty());
        assert!(results[0].snippet.contains("..."));
    }
}
