use crate::tools::index::IndexChunk;

/// Result of a codebase search query.
#[derive(Debug)]
pub struct SearchResult {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub kind_label: String,
    pub name: String,
    pub score: f64,
    pub snippet: String,
}

/// Maximum number of search results, configurable via `VEX_SEARCH_MAX_RESULTS`.
fn max_results_default() -> usize {
    std::env::var("VEX_SEARCH_MAX_RESULTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
}

/// Search the structural index for items matching `query`.
///
/// Ranking:
/// - Exact name match: +100
/// - Case-insensitive name contains query: +50
/// - Parent scope contains query: +20
/// - Content keyword match (per word): +5
///
/// Results are sorted by score descending, capped at `max_results`
/// (or `VEX_SEARCH_MAX_RESULTS` if `None`).
pub fn codebase_search(
    query: &str,
    index: &[IndexChunk],
    max_results: Option<usize>,
) -> Vec<SearchResult> {
    let cap = max_results.unwrap_or_else(max_results_default);
    if query.is_empty() || index.is_empty() {
        return Vec::new();
    }

    let query_lower = query.to_ascii_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();

    let mut scored: Vec<(f64, usize)> = index
        .iter()
        .enumerate()
        .filter_map(|(i, chunk)| {
            let score = score_chunk(chunk, &query_lower, &query_words);
            if score > 0.0 {
                Some((score, i))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(cap);

    scored
        .into_iter()
        .map(|(score, i)| {
            let chunk = &index[i];
            SearchResult {
                path: chunk.path.clone(),
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                kind_label: chunk.kind.label().to_string(),
                name: chunk.name.clone(),
                score,
                snippet: truncate_snippet(&chunk.source, 20),
            }
        })
        .collect()
}

fn score_chunk(chunk: &IndexChunk, query_lower: &str, query_words: &[&str]) -> f64 {
    let mut score = 0.0;
    let name_lower = chunk.name.to_ascii_lowercase();

    // Exact name match.
    if name_lower == *query_lower {
        score += 100.0;
    }
    // Case-insensitive name contains query.
    else if name_lower.contains(query_lower) {
        score += 50.0;
    }
    // Query contains name (useful when query is longer than the identifier).
    else if query_lower.contains(&name_lower) && name_lower.len() > 2 {
        score += 30.0;
    }

    // Parent scope match.
    if let Some(ref scope) = chunk.parent_scope {
        let scope_lower = scope.to_ascii_lowercase();
        if scope_lower.contains(query_lower) || query_lower.contains(&scope_lower) {
            score += 20.0;
        }
    }

    // Content keyword match.
    let source_lower = chunk.source.to_ascii_lowercase();
    for word in query_words {
        if word.len() >= 3 {
            let count = source_lower.matches(word).count();
            score += count as f64 * 5.0;
        }
    }

    score
}

/// Truncate a snippet to at most `max_lines` lines.
fn truncate_snippet(source: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if lines.len() <= max_lines {
        source.to_string()
    } else {
        let mut out: String = lines[..max_lines].join("\n");
        out.push_str(&format!(
            "\n    ... ({} more lines)",
            lines.len() - max_lines
        ));
        out
    }
}

/// Format search results for display as a tool response.
pub fn format_search_results(query: &str, results: &[SearchResult]) -> String {
    if results.is_empty() {
        return format!("No results found for \"{query}\".");
    }

    let mut out = format!("Found {} results for \"{}\":\n", results.len(), query);
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. [{}] `{}` in {}:{}-{}\n",
            i + 1,
            r.kind_label,
            r.name,
            r.path,
            r.start_line,
            r.end_line,
        ));
        // Indent snippet.
        for line in r.snippet.lines() {
            out.push_str("   ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::index::{IndexChunk, ItemKind};

    fn make_chunk(name: &str, kind: ItemKind, source: &str) -> IndexChunk {
        IndexChunk {
            path: "src/lib.rs".to_string(),
            start_line: 1,
            end_line: 5,
            kind,
            name: name.to_string(),
            parent_scope: None,
            source: source.to_string(),
        }
    }

    #[test]
    fn test_exact_name_match_ranks_highest() {
        let index = vec![
            make_chunk("build_index", ItemKind::Function, "fn build_index() {}"),
            make_chunk("rebuild", ItemKind::Function, "fn rebuild() {}"),
        ];
        let results = codebase_search("build_index", &index, Some(10));
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "build_index");
        assert!(results[0].score > results.get(1).map_or(0.0, |r| r.score));
    }

    #[test]
    fn test_empty_query_returns_empty() {
        let index = vec![make_chunk("foo", ItemKind::Function, "fn foo() {}")];
        let results = codebase_search("", &index, Some(10));
        assert!(results.is_empty());
    }

    #[test]
    fn test_max_results_caps_output() {
        let index: Vec<IndexChunk> = (0..20)
            .map(|i| {
                make_chunk(
                    &format!("fn_{i}"),
                    ItemKind::Function,
                    &format!("fn fn_{i}() {{}}"),
                )
            })
            .collect();
        let results = codebase_search("fn", &index, Some(5));
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_keyword_match_in_body() {
        let index = vec![
            make_chunk(
                "handler",
                ItemKind::Function,
                "fn handler() { process_request(); }",
            ),
            make_chunk("other", ItemKind::Function, "fn other() { nothing(); }"),
        ];
        let results = codebase_search("process_request", &index, Some(10));
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "handler");
    }

    #[test]
    fn test_format_search_results_empty() {
        let out = format_search_results("foo", &[]);
        assert!(out.contains("No results found"));
    }
}
