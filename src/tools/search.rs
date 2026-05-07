use crate::tools::index::IndexChunk;
use crate::tools::semantic::SemanticChunkScore;
use bm25::{Document as Bm25Document, Language, SearchEngineBuilder};
use bstr::ByteSlice;
use grep_regex::RegexMatcher;
use grep_searcher::{SearcherBuilder, sinks::UTF8};
use memmap2::Mmap;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Default)]
struct ParsedQuery {
    terms: Vec<String>,
    required_terms: Vec<String>,
    excluded_terms: Vec<String>,
    path_filters: Vec<String>,
    kind_filters: Vec<String>,
    lang_filters: Vec<String>,
}

impl ParsedQuery {
    fn parse(query: &str) -> Self {
        let mut parsed = Self::default();

        for token in tokenize_query(query) {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(path_filter) = trimmed.strip_prefix("path:") {
                let lowered = path_filter.trim().to_ascii_lowercase();
                if !lowered.is_empty() {
                    parsed.path_filters.push(lowered);
                }
                continue;
            }

            if let Some(kind_filter) = trimmed.strip_prefix("kind:") {
                if let Some(normalized) = normalize_kind_filter(kind_filter) {
                    parsed.kind_filters.push(normalized);
                }
                continue;
            }

            if let Some(lang_filter) = trimmed.strip_prefix("lang:") {
                if let Some(normalized) = normalize_lang_filter(lang_filter) {
                    parsed.lang_filters.push(normalized);
                }
                continue;
            }

            let (bucket, term) = if let Some(term) = trimmed.strip_prefix('+') {
                (&mut parsed.required_terms, term)
            } else if let Some(term) = trimmed.strip_prefix('-') {
                (&mut parsed.excluded_terms, term)
            } else {
                (&mut parsed.terms, trimmed)
            };

            let normalized = term.trim().to_ascii_lowercase();
            if !normalized.is_empty() {
                bucket.push(normalized);
            }
        }

        parsed
    }

    fn positive_query_text(&self) -> String {
        self.terms
            .iter()
            .chain(self.required_terms.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn has_positive_terms(&self) -> bool {
        !self.terms.is_empty() || !self.required_terms.is_empty()
    }
}

#[derive(Debug)]
struct ChunkSearchFields {
    name_lower: String,
    scope_lower: Option<String>,
    source_lower: String,
    path_lower: String,
    kind_lower: String,
    lang_lower: Option<&'static str>,
}

impl ChunkSearchFields {
    fn from_chunk(chunk: &IndexChunk) -> Self {
        Self {
            name_lower: chunk.name.to_ascii_lowercase(),
            scope_lower: chunk
                .parent_scope
                .as_ref()
                .map(|scope| scope.to_ascii_lowercase()),
            source_lower: chunk.source.to_ascii_lowercase(),
            path_lower: chunk.path.to_ascii_lowercase(),
            kind_lower: chunk.kind.label().to_ascii_lowercase(),
            lang_lower: detect_chunk_language(&chunk.path),
        }
    }

    fn matches_term(&self, term: &str) -> bool {
        self.name_lower.contains(term)
            || self.path_lower.contains(term)
            || self.kind_lower.contains(term)
            || self.source_lower.contains(term)
            || self
                .scope_lower
                .as_deref()
                .is_some_and(|scope| scope.contains(term))
            || self.lang_lower.is_some_and(|lang| lang == term)
    }
}

fn max_results_default() -> usize {
    std::env::var("VEX_SEARCH_MAX_RESULTS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(10)
}

fn build_bm25_engine(index: &[IndexChunk]) -> bm25::SearchEngine<usize> {
    let docs = index
        .iter()
        .enumerate()
        .map(|(i, chunk)| Bm25Document::new(i, format!("{} {}", chunk.name, chunk.source)));
    SearchEngineBuilder::<usize>::with_documents(Language::English, docs).build()
}

pub fn codebase_search(
    query: &str,
    index: &[IndexChunk],
    max_results: Option<usize>,
) -> Vec<SearchResult> {
    let cap = max_results.unwrap_or_else(max_results_default);
    if query.is_empty() || index.is_empty() {
        return Vec::new();
    }

    let parsed_query = ParsedQuery::parse(query);
    let positive_query = parsed_query.positive_query_text();

    let scored: Vec<(f64, usize)> = index
        .iter()
        .enumerate()
        .filter_map(|(i, chunk)| {
            let score = score_chunk(chunk, &parsed_query, &positive_query);
            if score > 0.0 { Some((score, i)) } else { None }
        })
        .collect();

    let bm25_scores: HashMap<usize, f64> = if positive_query.is_empty() {
        HashMap::new()
    } else {
        let engine = build_bm25_engine(index);
        engine
            .search(&positive_query, None)
            .into_iter()
            .filter_map(|r| {
                let chunk = index.get(r.document.id)?;
                let fields = ChunkSearchFields::from_chunk(chunk);
                if !chunk_matches_query(chunk, &fields, &parsed_query) {
                    return None;
                }
                if !matches_positive_terms(&fields, &parsed_query) {
                    return None;
                }
                let w = r.score as f64 * 10.0;
                if w > 0.0 {
                    Some((r.document.id, w))
                } else {
                    None
                }
            })
            .collect()
    };

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

fn tokenize_query(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in query.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn normalize_kind_filter(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" => None,
        "function" | "struct" | "class" | "interface" | "enum" | "impl" | "trait" | "module"
        | "const" | "static" | "type" => Some(normalized),
        "typealias" => Some("type".to_string()),
        _ => Some(normalized),
    }
}

fn normalize_lang_filter(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_lowercase();
    let canonical = match normalized.as_str() {
        "" => return None,
        "rs" | "rust" => "rust",
        "py" | "python" => "python",
        "ts" | "tsx" | "typescript" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" | "javascript" => "javascript",
        _ => normalized.as_str(),
    };
    Some(canonical.to_string())
}

fn detect_chunk_language(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript"),
        _ => None,
    }
}

fn chunk_matches_query(
    _chunk: &IndexChunk,
    fields: &ChunkSearchFields,
    parsed_query: &ParsedQuery,
) -> bool {
    if !parsed_query.path_filters.is_empty()
        && !parsed_query
            .path_filters
            .iter()
            .any(|path_filter| fields.path_lower.contains(path_filter))
    {
        return false;
    }

    if !parsed_query.kind_filters.is_empty()
        && !parsed_query
            .kind_filters
            .iter()
            .any(|kind_filter| kind_filter == &fields.kind_lower)
    {
        return false;
    }

    if !parsed_query.lang_filters.is_empty()
        && !parsed_query
            .lang_filters
            .iter()
            .any(|lang_filter| fields.lang_lower.is_some_and(|lang| lang == lang_filter))
    {
        return false;
    }

    if parsed_query
        .excluded_terms
        .iter()
        .any(|excluded| fields.matches_term(excluded))
    {
        return false;
    }

    if parsed_query
        .required_terms
        .iter()
        .any(|required| !fields.matches_term(required))
    {
        return false;
    }

    true
}

fn matches_positive_terms(fields: &ChunkSearchFields, parsed_query: &ParsedQuery) -> bool {
    if !parsed_query.required_terms.is_empty()
        && parsed_query
            .required_terms
            .iter()
            .any(|required| !fields.matches_term(required))
    {
        return false;
    }

    if parsed_query.terms.is_empty() {
        return true;
    }

    parsed_query
        .terms
        .iter()
        .any(|term| fields.matches_term(term))
}

fn score_term(fields: &ChunkSearchFields, term: &str, required: bool) -> f64 {
    let mut score = 0.0;
    let source_match_weight = if term.contains(' ') { 8.0 } else { 5.0 };
    let exact_name_weight = if required { 60.0 } else { 45.0 };
    let substring_name_weight = if required { 25.0 } else { 15.0 };

    if fields.name_lower == term {
        score += exact_name_weight;
    } else if fields.name_lower.contains(term) {
        score += substring_name_weight;
    }

    if fields.path_lower.contains(term) {
        score += 10.0;
    }

    if fields.kind_lower == term {
        score += 10.0;
    }

    if let Some(scope_lower) = fields.scope_lower.as_deref()
        && scope_lower.contains(term)
    {
        score += 12.0;
    }

    let count = fields.source_lower.matches(term).count();
    if count > 0 {
        score += count as f64 * source_match_weight;
    }

    score
}

fn score_chunk(chunk: &IndexChunk, parsed_query: &ParsedQuery, positive_query: &str) -> f64 {
    let fields = ChunkSearchFields::from_chunk(chunk);
    if !chunk_matches_query(chunk, &fields, parsed_query) {
        return 0.0;
    }

    let mut score = 0.0;
    let query_lower = positive_query.to_ascii_lowercase();

    if !query_lower.is_empty() {
        if fields.name_lower == query_lower {
            score += 100.0;
        } else if fields.name_lower.contains(&query_lower) {
            score += 50.0;
        } else if query_lower.contains(&fields.name_lower) && fields.name_lower.len() > 2 {
            score += 30.0;
        }

        if let Some(scope_lower) = fields.scope_lower.as_deref()
            && (scope_lower.contains(&query_lower) || query_lower.contains(scope_lower))
        {
            score += 20.0;
        }
    }

    for term in &parsed_query.terms {
        score += score_term(&fields, term, false);
    }

    for term in &parsed_query.required_terms {
        score += score_term(&fields, term, true);
    }

    if score <= 0.0 && !parsed_query.has_positive_terms() {
        return 1.0;
    }

    score
}

fn semantic_score_weight(score: f64) -> f64 {
    score.max(0.0) * 60.0
}

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

        for line in r.snippet.lines() {
            out.push_str("   ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

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
        Err(_) => Vec::new(),
    }
}

#[allow(unsafe_code)]
pub fn mmap_read_file(path: &Path) -> Option<Mmap> {
    let file = File::open(path).ok()?;
    unsafe { Mmap::map(&file).ok() }
}

pub fn is_binary_content(bytes: &[u8]) -> bool {
    let probe = &bytes[..bytes.len().min(8192)];
    probe.find_byte(b'\0').is_some()
}

pub fn parallel_search_files(paths: &[PathBuf], pattern: &str) -> Vec<(String, u64, String)> {
    use crossbeam_channel::unbounded;
    let (tx, rx) = unbounded::<(String, u64, String)>();

    paths.par_iter().for_each(|path| {
        let tx = tx.clone();

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

    drop(tx);
    rx.into_iter().collect()
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

    fn make_chunk_at(path: &str, name: &str, kind: ItemKind, source: &str) -> IndexChunk {
        IndexChunk {
            path: path.to_string(),
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
    fn test_filter_only_query_returns_matching_chunks() {
        let index = vec![
            make_chunk_at(
                "src/auth.rs",
                "login_handler",
                ItemKind::Function,
                "fn login_handler() {}",
            ),
            make_chunk_at(
                "src/model.rs",
                "LoginState",
                ItemKind::Struct,
                "struct LoginState;",
            ),
        ];

        let results = codebase_search("path:src/auth.rs kind:function", &index, Some(10));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "login_handler");
    }

    #[test]
    fn test_path_and_kind_filters_limit_results() {
        let index = vec![
            make_chunk_at(
                "src/auth.rs",
                "login_handler",
                ItemKind::Function,
                "fn login_handler() { authenticate(); }",
            ),
            make_chunk_at(
                "src/ui.rs",
                "login_handler",
                ItemKind::Function,
                "fn login_handler() { render(); }",
            ),
            make_chunk_at(
                "src/auth.rs",
                "LoginHandler",
                ItemKind::Struct,
                "struct LoginHandler;",
            ),
        ];

        let results = codebase_search("login path:src/auth kind:function", &index, Some(10));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "src/auth.rs");
        assert_eq!(results[0].kind_label, "function");
    }

    #[test]
    fn test_lang_filter_uses_file_extension() {
        let index = vec![
            make_chunk_at(
                "src/engine.rs",
                "parse_config",
                ItemKind::Function,
                "fn parse_config() -> Result<()> { Ok(()) }",
            ),
            make_chunk_at(
                "src/engine.py",
                "parse_config",
                ItemKind::Function,
                "def parse_config():\n    return True",
            ),
        ];

        let results = codebase_search("parse_config lang:python", &index, Some(10));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "src/engine.py");
    }

    #[test]
    fn test_required_and_excluded_terms_are_enforced() {
        let index = vec![
            make_chunk_at(
                "src/config.rs",
                "reload_config",
                ItemKind::Function,
                "fn reload_config() { apply_reload(); }",
            ),
            make_chunk_at(
                "src/legacy.rs",
                "reload_config_legacy",
                ItemKind::Function,
                "fn reload_config_legacy() { legacy_reload(); }",
            ),
        ];

        let results = codebase_search("+reload -legacy", &index, Some(10));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "reload_config");
    }

    #[test]
    fn test_quoted_phrase_matches_adjacent_terms() {
        let index = vec![
            make_chunk_at(
                "src/auth.rs",
                "login_handler",
                ItemKind::Function,
                "fn login_handler() { /* process request */ }",
            ),
            make_chunk_at(
                "src/other.rs",
                "reverse_handler",
                ItemKind::Function,
                "fn reverse_handler() { /* request process */ }",
            ),
        ];

        let results = codebase_search("\"process request\"", &index, Some(10));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "login_handler");
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

    #[test]
    fn test_codebase_search_tool_returns_ranked_results() {
        let index = vec![
            make_chunk(
                "ranked_handler",
                ItemKind::Function,
                "fn ranked_handler() { route_request(); }",
            ),
            make_chunk(
                "unrelated_util",
                ItemKind::Function,
                "fn unrelated_util() {}",
            ),
            make_chunk("route_request", ItemKind::Function, "fn route_request() {}"),
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
}
