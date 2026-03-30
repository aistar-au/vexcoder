use std::fs;
use std::path::Path;

/// Kind of a source-level item extracted by the structural index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    Function,
    Struct,
    Enum,
    Impl,
    Trait,
    Module,
    Const,
    Static,
    TypeAlias,
}

impl ItemKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Impl => "impl",
            Self::Trait => "trait",
            Self::Module => "module",
            Self::Const => "const",
            Self::Static => "static",
            Self::TypeAlias => "type",
        }
    }
}

/// A single indexed code chunk extracted from a Rust source file.
#[derive(Debug, Clone)]
pub struct IndexChunk {
    /// Workspace-relative path (forward slashes).
    pub path: String,
    /// 1-based start line.
    pub start_line: usize,
    /// 1-based end line (inclusive).
    pub end_line: usize,
    pub kind: ItemKind,
    pub name: String,
    pub parent_scope: Option<String>,
    pub source: String,
}

/// Build a structural index of all `.rs` files under `workspace_root`.
///
/// Walks the source tree, parses each Rust file with Tree-sitter, and extracts
/// named items (functions, structs, enums, impls, traits, modules, consts,
/// statics, type aliases).
///
/// `exclude` is a list of workspace-relative path prefixes (e.g. `"target/"`)
/// to skip.  `max_file_size` sets the byte limit above which files are ignored.
/// Pass `&[]` and `usize::MAX` to apply no filtering.
pub fn build_index(workspace_root: &Path) -> Vec<IndexChunk> {
    build_index_filtered(workspace_root, &[], usize::MAX)
}

/// Like [`build_index`] but respects caller-supplied exclusion rules.
pub fn build_index_filtered(
    workspace_root: &Path,
    exclude: &[String],
    max_file_size: usize,
) -> Vec<IndexChunk> {
    let mut chunks = Vec::new();
    let src_dir = workspace_root.join("src");
    if !src_dir.is_dir() {
        return chunks;
    }
    collect_rs_files_filtered(
        &src_dir,
        workspace_root,
        exclude,
        max_file_size,
        &mut chunks,
    );
    chunks
}

/// Re-index a single file: remove old chunks for that path and re-parse.
pub fn update_index(index: &mut Vec<IndexChunk>, changed_path: &Path, workspace_root: &Path) {
    update_index_filtered(index, changed_path, workspace_root, &[], usize::MAX);
}

/// Re-index a single file with caller-supplied exclusion and size filters.
pub fn update_index_filtered(
    index: &mut Vec<IndexChunk>,
    changed_path: &Path,
    workspace_root: &Path,
    exclude: &[String],
    max_file_size: usize,
) {
    let rel = workspace_relative(changed_path, workspace_root);
    index.retain(|c| c.path != rel);
    if exclude.iter().any(|ex| rel.starts_with(ex.as_str())) {
        return;
    }
    if changed_path.extension().is_some_and(|e| e == "rs") {
        let size = changed_path
            .metadata()
            .map(|m| m.len() as usize)
            .unwrap_or(0);
        if size > max_file_size {
            return;
        }
        if let Ok(source) = fs::read_to_string(changed_path) {
            parse_rust_file(&rel, &source, index);
        }
    }
}

fn collect_rs_files_filtered(
    dir: &Path,
    workspace_root: &Path,
    exclude: &[String],
    max_file_size: usize,
    chunks: &mut Vec<IndexChunk>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let rel = workspace_relative(&path, workspace_root);
        if exclude.iter().any(|ex| rel.starts_with(ex.as_str())) {
            continue;
        }
        if path.is_dir() {
            collect_rs_files_filtered(&path, workspace_root, exclude, max_file_size, chunks);
        } else if path.extension().is_some_and(|e| e == "rs") {
            // Skip files exceeding the size limit.
            let size = path.metadata().map(|m| m.len() as usize).unwrap_or(0);
            if size > max_file_size {
                continue;
            }
            if let Ok(source) = fs::read_to_string(&path) {
                parse_rust_file(&rel, &source, chunks);
            }
        }
    }
}

fn workspace_relative(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Parse a single Rust source file and extract structural items.
fn parse_rust_file(rel_path: &str, source: &str, chunks: &mut Vec<IndexChunk>) {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_rust::LANGUAGE;
    if parser.set_language(&language.into()).is_err() {
        return;
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return,
    };
    let source_bytes = source.as_bytes();
    extract_items(tree.root_node(), source_bytes, rel_path, None, chunks);
}

fn extract_items(
    node: tree_sitter::Node,
    source: &[u8],
    rel_path: &str,
    parent_scope: Option<&str>,
    chunks: &mut Vec<IndexChunk>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let kind_str = child.kind();
        let item_kind = match kind_str {
            "function_item" => Some(ItemKind::Function),
            "struct_item" => Some(ItemKind::Struct),
            "enum_item" => Some(ItemKind::Enum),
            "impl_item" => Some(ItemKind::Impl),
            "trait_item" => Some(ItemKind::Trait),
            "mod_item" => Some(ItemKind::Module),
            "const_item" => Some(ItemKind::Const),
            "static_item" => Some(ItemKind::Static),
            "type_item" => Some(ItemKind::TypeAlias),
            _ => None,
        };

        if let Some(kind) = item_kind {
            let name = extract_name(child, source, &kind);
            let start_line = child.start_position().row + 1;
            let end_line = child.end_position().row + 1;
            let chunk_source = child.utf8_text(source).unwrap_or("").to_string();

            let scope_name =
                if kind == ItemKind::Impl || kind == ItemKind::Trait || kind == ItemKind::Module {
                    Some(name.clone())
                } else {
                    parent_scope.map(String::from)
                };

            chunks.push(IndexChunk {
                path: rel_path.to_string(),
                start_line,
                end_line,
                kind: kind.clone(),
                name,
                parent_scope: scope_name.clone(),
                source: chunk_source,
            });

            // Recurse into impl/trait/mod bodies for nested items.
            if kind == ItemKind::Impl || kind == ItemKind::Trait || kind == ItemKind::Module {
                let scope = scope_name.as_deref();
                extract_items(child, source, rel_path, scope, chunks);
            }
        } else if kind_str == "declaration_list" {
            // Recurse into declaration_list (body of impl/trait/mod blocks).
            extract_items(child, source, rel_path, parent_scope, chunks);
        }
    }
}

fn extract_name(node: tree_sitter::Node, source: &[u8], kind: &ItemKind) -> String {
    // For impl blocks, look for the type being implemented.
    if *kind == ItemKind::Impl {
        // Try to find `type_identifier` child for `impl Foo { ... }`
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier" || child.kind() == "generic_type" {
                return child.utf8_text(source).unwrap_or("_").to_string();
            }
        }
        return "_impl".to_string();
    }

    // For other items, look for the `name` field or first `identifier`/`type_identifier`.
    if let Some(name_node) = node.child_by_field_name("name") {
        return name_node.utf8_text(source).unwrap_or("_").to_string();
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" || child.kind() == "type_identifier" {
            return child.utf8_text(source).unwrap_or("_").to_string();
        }
    }
    "_".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_rust_file_extracts_function() {
        let source = "pub fn hello_world() -> i32 { 42 }\n";
        let mut chunks = Vec::new();
        parse_rust_file("test.rs", source, &mut chunks);
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].kind, ItemKind::Function);
        assert_eq!(chunks[0].name, "hello_world");
        assert_eq!(chunks[0].start_line, 1);
    }

    #[test]
    fn test_parse_rust_file_extracts_struct() {
        let source = "pub struct Foo {\n    bar: i32,\n}\n";
        let mut chunks = Vec::new();
        parse_rust_file("test.rs", source, &mut chunks);
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].kind, ItemKind::Struct);
        assert_eq!(chunks[0].name, "Foo");
    }

    #[test]
    fn test_parse_rust_file_extracts_impl_methods() {
        let source = "struct Foo;\nimpl Foo {\n    fn bar(&self) {}\n    fn baz(&self) {}\n}\n";
        let mut chunks = Vec::new();
        parse_rust_file("test.rs", source, &mut chunks);
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Foo")); // struct + impl
        assert!(names.contains(&"bar"));
        assert!(names.contains(&"baz"));
    }

    #[test]
    fn test_build_index_with_tempdir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("mkdir");
        let mut f = fs::File::create(src.join("main.rs")).expect("create");
        f.write_all(b"fn main() {}\npub struct App;\n")
            .expect("write");
        drop(f);

        let chunks = build_index(tmp.path());
        assert!(chunks.len() >= 2);
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"App"));
    }

    #[test]
    fn test_update_index_replaces_file_chunks() {
        let mut chunks = vec![IndexChunk {
            path: "src/foo.rs".to_string(),
            start_line: 1,
            end_line: 3,
            kind: ItemKind::Function,
            name: "old_fn".to_string(),
            parent_scope: None,
            source: "fn old_fn() {}".to_string(),
        }];

        let tmp = tempfile::tempdir().expect("tempdir");
        let file_path = tmp.path().join("src").join("foo.rs");
        fs::create_dir_all(file_path.parent().unwrap()).expect("mkdir");
        fs::write(&file_path, "fn new_fn() {}\n").expect("write");

        update_index(&mut chunks, &file_path, tmp.path());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].name, "new_fn");
    }

    #[test]
    fn test_mod_item_propagates_scope_to_nested_functions() {
        let source = "mod mymod {\n    fn inner() {}\n}\n";
        let mut chunks = Vec::new();
        parse_rust_file("test.rs", source, &mut chunks);
        let inner = chunks.iter().find(|c| c.name == "inner");
        assert!(inner.is_some(), "expected to find function 'inner'");
        assert_eq!(
            inner.unwrap().parent_scope.as_deref(),
            Some("mymod"),
            "function inside mod should have parent_scope = module name"
        );
    }

    /// Anchor test: `build_index_filtered` must not index files under
    /// workspace-relative paths that appear in the exclusion list.
    #[test]
    fn test_search_config_respects_exclude_paths() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Create src/lib.rs — should be indexed.
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("mkdir src");
        fs::write(src.join("lib.rs"), "pub fn included_fn() {}\n").expect("write lib.rs");

        // Create src/vendor/mod.rs — excluded by "src/vendor/" prefix.
        let vendor = src.join("vendor");
        fs::create_dir_all(&vendor).expect("mkdir vendor");
        fs::write(vendor.join("mod.rs"), "pub fn excluded_fn() {}\n").expect("write vendor/mod.rs");

        let exclude = vec!["src/vendor/".to_string()];
        let chunks = build_index_filtered(tmp.path(), &exclude, usize::MAX);

        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"included_fn"),
            "non-excluded files must be indexed"
        );
        assert!(
            !names.contains(&"excluded_fn"),
            "files under excluded prefix must not appear in index"
        );
    }

    /// Regression: exclude prefix with trailing slash must not match
    /// directories that merely share a common stem (e.g. `"src/data/"`
    /// must not match `"src/data_backup/lib.rs"`).  The config layer
    /// normalizes entries to include a trailing slash.
    #[test]
    fn exclude_prefix_requires_path_boundary() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data = tmp.path().join("src").join("data");
        let data_backup = tmp.path().join("src").join("data_backup");
        fs::create_dir_all(&data).expect("mkdir data");
        fs::create_dir_all(&data_backup).expect("mkdir data_backup");
        fs::write(data.join("lib.rs"), "pub fn in_data() {}\n").expect("write");
        fs::write(data_backup.join("lib.rs"), "pub fn in_data_backup() {}\n").expect("write");

        // With trailing slash — only src/data/ is excluded, not src/data_backup/.
        let chunks = build_index_filtered(tmp.path(), &["src/data/".to_string()], usize::MAX);
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"in_data"), "src/data/ must be excluded");
        assert!(
            names.contains(&"in_data_backup"),
            "src/data_backup/ must NOT be excluded by src/data/ prefix"
        );
    }

    /// Anchor test: a file write followed by `update_index` must refresh the
    /// index incrementally without a full rebuild.
    #[test]
    fn test_incremental_update_after_write_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("mkdir");
        let file_path = src.join("incremental.rs");
        fs::write(&file_path, "fn before_write() {}\n").expect("write");

        let mut chunks = build_index(tmp.path());
        assert!(
            chunks.iter().any(|c| c.name == "before_write"),
            "initial index must contain before_write"
        );

        // Simulate an external file write that changes the symbol.
        fs::write(&file_path, "fn after_write() {}\n").expect("overwrite");
        update_index(&mut chunks, &file_path, tmp.path());

        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"after_write"),
            "updated symbol must appear in index after incremental refresh"
        );
        assert!(
            !names.contains(&"before_write"),
            "stale symbol must be replaced by incremental refresh"
        );
    }

    #[test]
    fn test_update_index_filtered_respects_excluded_paths() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        let vendor = src.join("vendor");
        fs::create_dir_all(&vendor).expect("mkdir vendor");
        fs::write(src.join("lib.rs"), "pub fn included_fn() {}\n").expect("write lib.rs");

        let mut chunks = build_index_filtered(tmp.path(), &[], usize::MAX);
        let vendor_file = vendor.join("mod.rs");
        fs::write(&vendor_file, "pub fn excluded_incremental() {}\n").expect("write vendor");

        update_index_filtered(
            &mut chunks,
            &vendor_file,
            tmp.path(),
            &["src/vendor/".to_string()],
            usize::MAX,
        );

        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"included_fn"));
        assert!(
            !names.contains(&"excluded_incremental"),
            "incremental refresh must skip excluded paths"
        );
    }

    #[test]
    fn test_update_index_filtered_respects_max_file_size() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("mkdir");
        fs::write(src.join("small.rs"), "fn small_fn() {}\n").expect("write small.rs");

        let mut chunks = build_index_filtered(tmp.path(), &[], 30);
        let large_path = src.join("large.rs");
        fs::write(
            &large_path,
            "fn large_fn_that_is_too_big_for_incremental_cap() {}\n",
        )
        .expect("write large.rs");

        update_index_filtered(&mut chunks, &large_path, tmp.path(), &[], 30);

        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"small_fn"));
        assert!(
            !names.contains(&"large_fn_that_is_too_big_for_incremental_cap"),
            "incremental refresh must skip oversized files"
        );
    }

    /// Anchor test: `build_index_filtered` must not index files whose byte size
    /// exceeds the configured `max_file_size` limit.
    #[test]
    fn test_build_index_filtered_skips_oversized_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("mkdir");

        // Small file that should be indexed.
        fs::write(src.join("small.rs"), "fn small_fn() {}\n").expect("write small.rs");
        // Larger file (~47 bytes), will be excluded with a 30-byte cap.
        fs::write(
            src.join("large.rs"),
            "fn large_fn_that_is_too_big_for_this_cap() {}\n",
        )
        .expect("write large.rs");

        let chunks = build_index_filtered(tmp.path(), &[], 30);
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"small_fn"),
            "small file must be indexed when under size cap"
        );
        assert!(
            !names.contains(&"large_fn_that_is_too_big_for_this_cap"),
            "file exceeding max_file_size must be skipped"
        );
    }

    /// Anchor test: forcing a full reindex via the public helper must rebuild the
    /// index from the workspace and return a non-zero chunk count.
    #[test]
    fn test_reindex_rebuilds_full_index() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("mkdir");
        fs::write(
            src.join("reindex_target.rs"),
            "pub fn reindex_symbol() {}\n",
        )
        .expect("write");

        // Build via the filtered entry-point (the same path used by force_full_reindex).
        let chunks = build_index_filtered(tmp.path(), &[], usize::MAX);
        assert!(
            !chunks.is_empty(),
            "rebuild must produce at least one chunk"
        );
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"reindex_symbol"),
            "known symbol must be present after full reindex"
        );
    }
}
