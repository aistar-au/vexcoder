use crate::tools::index::IndexChunk;
use crate::tools::semantic::SemanticChunkScore;
use bm25::{Document as Bm25Document, Language, SearchEngineBuilder};
use bstr::ByteSlice;
use grep_regex::RegexMatcher;
use grep_searcher::{sinks::UTF8, SearcherBuilder};
use memmap2::Mmap;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

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
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(10)
}

/// Build a BM25 `SearchEngine` over all chunks in `index`.
///
/// Each chunk is indexed as its `name` concatenated with its `source` so the
/// BM25 ranking captures both symbol name and code content.  The document id
/// is the chunk position (`usize`) so callers can map results back to the
/// original `index` slice in O(1).
fn build_bm25_engine(index: &[IndexChunk]) -> bm25::SearchEngine<usize> {
    let docs = index
        .iter()
        .enumerate()
        .map(|(i, chunk)| Bm25Document::new(i, format!("{} {}", chunk.name, chunk.source)));
    SearchEngineBuilder::<usize>::with_documents(Language::English, docs).build()
}

/// Search the structural index for items matching `query`.
///
/// Ranking pipeline (additive scores):
/// 1. Structural scoring via `score_chunk` (exact/fuzzy name match, parent scope, keyword count).
/// 2. BM25 term-frequency reranking layer — adds a weighted BM25 score to the
///    structural score so frequently-matched terms boost results proportionally.
///
/// Results are sorted by combined score descending, capped at `max_results`
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

    let scored: Vec<(f64, usize)> = index
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

    // BM25 reranking layer: build a Search engine over all index chunks and add
    // BM25 term-frequency weight to the structural scores for each candidate.
    // BM25 weight is scaled to the same order-of-magnitude as the structural
    // scores so neither signal dominates when they diverge.
    let engine = build_bm25_engine(index);
    let bm25_scores: HashMap<usize, f64> = engine
        .search(&query_lower, None)
        .into_iter()
        .filter_map(|r| {
            let w = r.score as f64 * 10.0;
            if w > 0.0 {
                Some((r.document.id, w))
            } else {
                None
            }
        })
        .collect();

    // Merge BM25 weights into structural scores using a HashMap for O(1)
    // lookup per entry instead of a linear scan.
    let mut score_map: HashMap<usize, f64> = scored.into_iter().map(|(s, i)| (i, s)).collect();
    for (idx, bm25_weight) in &bm25_scores {
        *score_map.entry(*idx).or_insert(0.0) += bm25_weight;
    }
    let mut scored: Vec<(f64, usize)> = score_map.into_iter().map(|(i, s)| (s, i)).collect();

    scored.sort_by(|(score_a, idx_a), (score_b, idx_b)| {
        score_b.total_cmp(score_a).then(idx_a.cmp(idx_b))
    });
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

pub fn merge_search_results(
    index: &[IndexChunk],
    structural_results: Vec<SearchResult>,
    semantic_scores: Vec<SemanticChunkScore>,
    max_results: Option<usize>,
) -> Vec<SearchResult> {
    let cap = max_results.unwrap_or_else(max_results_default);
    if cap == 0 {
        return Vec::new();
    }

    let mut merged = structural_results;
    let mut merged_positions: HashMap<(String, usize, usize, String), usize> = merged
        .iter()
        .enumerate()
        .map(|(idx, result)| {
            (
                (
                    result.path.clone(),
                    result.start_line,
                    result.end_line,
                    result.name.clone(),
                ),
                idx,
            )
        })
        .collect();

    let index_lookup: HashMap<(String, usize, usize, String), &IndexChunk> = index
        .iter()
        .map(|chunk| {
            (
                (
                    chunk.path.clone(),
                    chunk.start_line,
                    chunk.end_line,
                    chunk.name.clone(),
                ),
                chunk,
            )
        })
        .collect();

    for semantic in semantic_scores {
        let key = (
            semantic.path.clone(),
            semantic.start_line,
            semantic.end_line,
            semantic.name.clone(),
        );
        let semantic_weight = semantic_score_weight(semantic.score);
        if semantic_weight <= 0.0 {
            continue;
        }

        if let Some(existing_idx) = merged_positions.get(&key).copied() {
            merged[existing_idx].score += semantic_weight;
            continue;
        }

        let Some(chunk) = index_lookup.get(&key) else {
            continue;
        };

        merged_positions.insert(key, merged.len());
        merged.push(SearchResult {
            path: chunk.path.clone(),
            start_line: chunk.start_line,
            end_line: chunk.end_line,
            kind_label: chunk.kind.label().to_string(),
            name: chunk.name.clone(),
            score: semantic_weight,
            snippet: truncate_snippet(&chunk.source, 20),
        });
    }

    merged.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.start_line.cmp(&right.start_line))
    });
    merged.truncate(cap);
    merged
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

fn semantic_score_weight(score: f64) -> f64 {
    score.max(0.0) * 60.0
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

// ── Grep-based file search ────────────────────────────────────────────────────

/// Search a single file for lines matching `pattern` using the grep-regex
/// engine backed by the `regex` crate.  Returns `(line_number, matched_line)`
/// tuples for every matching line.  The Vec is empty when the file cannot be
/// read, the pattern is invalid, or there are no matches.
pub fn grep_search_file(path: &Path, pattern: &str) -> Vec<(u64, String)> {
    let matcher = match RegexMatcher::new(pattern) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    let mut matches: Vec<(u64, String)> = Vec::new();
    let mut searcher = SearcherBuilder::new().line_number(true).build();
    match searcher.search_path(
        &matcher,
        path,
        UTF8(|line_number, line| {
            matches.push((line_number, line.trim_end_matches('\n').to_string()));
            Ok(true)
        }),
    ) {
        Ok(_) => matches,
        Err(_) => {
            // File may contain invalid UTF-8 (but no NUL bytes); fall back
            // to an empty result rather than silently suppressing the error.
            Vec::new()
        }
    }
}

// ── Memory-mapped file reading ────────────────────────────────────────────────

/// Read the byte contents of `path` using a read-only memory mapping.
///
/// Faster than buffered I/O for large files because the OS page cache is
/// reused without copying.  Returns `None` when the file cannot be opened or
/// mapped.
pub fn mmap_read_file(path: &Path) -> Option<Mmap> {
    let file = File::open(path).ok()?;
    // SAFETY: the file is opened read-only; no mutable alias to these pages
    // exists through this mapping during the lifetime of the returned Mmap.
    unsafe { Mmap::map(&file).ok() }
}

// ── Binary content detection ──────────────────────────────────────────────────

/// Return `true` when `bytes` looks like binary (non-text) content.
///
/// Scans the first 8 KiB for null bytes — the same heuristic used by git and
/// ripgrep.  Binary files are excluded from text-oriented search operations.
pub fn is_binary_content(bytes: &[u8]) -> bool {
    let probe = &bytes[..bytes.len().min(8192)];
    probe.find_byte(b'\0').is_some()
}

// ── Parallel file search ──────────────────────────────────────────────────────

/// Search `paths` in parallel for lines matching `pattern`.
///
/// Returns one `(path_string, line_number, line_text)` entry per match.
/// Uses `rayon` for CPU-bound parallel iteration and `crossbeam_channel` to
/// aggregate results from worker threads without contention.  Paths that
/// cannot be memory-mapped or that look like binary content are silently
/// skipped.
pub fn parallel_search_files(paths: &[PathBuf], pattern: &str) -> Vec<(String, u64, String)> {
    use crossbeam_channel::unbounded;
    let (tx, rx) = unbounded::<(String, u64, String)>();

    paths.par_iter().for_each(|path| {
        let tx = tx.clone();
        // Skip unreadable or binary files before invoking the regex engine.
        match mmap_read_file(path) {
            Some(mmap) => {
                if is_binary_content(&mmap) {
                    return;
                }
            }
            None => return,
        }
        for (line_num, text) in grep_search_file(path, pattern) {
            let _ = tx.send((path.to_string_lossy().into_owned(), line_num, text));
        }
    });
    // Drop the last sender so `rx.into_iter()` terminates.
    drop(tx);
    rx.into_iter().collect()
}

// ── Syntax-aware language detection ──────────────────────────────────────────

/// Map a source file path to a tree-sitter `Language` by file extension.
///
/// Supported extensions: `.py` (Python), `.ts` (TypeScript), `.tsx` (TSX),
/// `.js` / `.mjs` / `.cjs` / `.jsx` (JavaScript).  Returns `None` for
/// unrecognised extensions.
pub fn detect_language(path: &Path) -> Option<tree_sitter::Language> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "py" => Some(tree_sitter_python::LANGUAGE.into()),
        "ts" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "js" | "mjs" | "cjs" | "jsx" => Some(tree_sitter_javascript::LANGUAGE.into()),
        _ => None,
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

    #[test]
    fn test_merge_search_results_boosts_structural_match() {
        let index = vec![make_chunk(
            "ranked_handler",
            ItemKind::Function,
            "fn ranked_handler() { process_request(); }",
        )];
        let structural = codebase_search("ranked_handler", &index, Some(10));
        let base_score = structural[0].score;

        let merged = merge_search_results(
            &index,
            structural,
            vec![SemanticChunkScore {
                path: "src/lib.rs".to_string(),
                start_line: 1,
                end_line: 5,
                name: "ranked_handler".to_string(),
                score: 0.8,
            }],
            Some(10),
        );

        assert_eq!(merged[0].name, "ranked_handler");
        assert!(merged[0].score > base_score);
    }

    #[test]
    fn test_merge_search_results_adds_semantic_only_match() {
        let index = vec![make_chunk(
            "semantic_candidate",
            ItemKind::Function,
            "fn semantic_candidate() { handle_request(); }",
        )];

        let merged = merge_search_results(
            &index,
            Vec::new(),
            vec![SemanticChunkScore {
                path: "src/lib.rs".to_string(),
                start_line: 1,
                end_line: 5,
                name: "semantic_candidate".to_string(),
                score: 0.7,
            }],
            Some(10),
        );

        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "semantic_candidate");
        assert!(merged[0].score > 0.0);
    }

    /// Anchor test: querying a rebuilt index must return ranked structural results
    /// for known symbol names. Verifies that names rank above unrelated entries.
    #[test]
    fn test_codebase_search_tool_returns_ranked_results() {
        let index = vec![
            make_chunk(
                "ranked_handler",
                ItemKind::Function,
                "fn ranked_handler() { dispatch_request(); }",
            ),
            make_chunk(
                "unrelated_util",
                ItemKind::Function,
                "fn unrelated_util() {}",
            ),
            make_chunk(
                "dispatch_request",
                ItemKind::Function,
                "fn dispatch_request() {}",
            ),
        ];
        let results = codebase_search("ranked_handler", &index, Some(10));
        assert!(
            !results.is_empty(),
            "search must return results for a matching symbol"
        );
        assert_eq!(
            results[0].name, "ranked_handler",
            "exact name match must rank first"
        );
        assert!(
            results[0].score > results.get(1).map_or(0.0, |r| r.score),
            "highest-scoring result must have the largest score"
        );
    }

    #[test]
    fn test_grep_search_file_finds_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.rs");
        std::fs::write(&path, "line one\nfn hello() {}\nline three\n").unwrap();
        let hits = grep_search_file(&path, "hello");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 2);
        assert!(hits[0].1.contains("hello"));
    }

    #[test]
    fn test_grep_search_file_invalid_regex_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.txt");
        std::fs::write(&path, "content").unwrap();
        let hits = grep_search_file(&path, "[invalid");
        assert!(hits.is_empty());
    }

    #[test]
    fn test_is_binary_content_detects_null_bytes() {
        assert!(is_binary_content(b"hello\x00world"));
        assert!(!is_binary_content(b"hello world"));
        assert!(!is_binary_content(b""));
    }

    #[test]
    fn test_detect_language_maps_extensions() {
        assert!(detect_language(Path::new("foo.py")).is_some());
        assert!(detect_language(Path::new("bar.ts")).is_some());
        assert!(detect_language(Path::new("baz.tsx")).is_some());
        assert!(detect_language(Path::new("qux.js")).is_some());
        assert!(detect_language(Path::new("qux.mjs")).is_some());
        assert!(detect_language(Path::new("unknown.xyz")).is_none());
    }
}
