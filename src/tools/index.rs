use std::fs;
use std::path::Path;

use crate::tools::operator::ToolOperator;
use crate::tools::workspace_ignore::WorkspaceIgnore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    Function,
    Struct,
    Class,
    Interface,
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
            Self::Class => "class",
            Self::Interface => "interface",
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

#[derive(Debug, Clone)]
pub struct IndexChunk {
    pub path: String,

    pub start_line: usize,

    pub end_line: usize,
    pub kind: ItemKind,
    pub name: String,
    pub parent_scope: Option<String>,
    pub source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceLanguage {
    Rust,
    Python,
    TypeScript,
    Tsx,
    JavaScript,
}

impl SourceLanguage {
    fn parser_language(self) -> tree_sitter::Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        }
    }
}

pub fn build_index(workspace_root: &Path) -> Vec<IndexChunk> {
    build_index_filtered(workspace_root, &[], usize::MAX)
}

pub fn build_index_filtered(
    workspace_root: &Path,
    exclude: &[String],
    max_file_size: usize,
) -> Vec<IndexChunk> {
    let operator = ToolOperator::new(workspace_root.to_path_buf());
    let ignore = WorkspaceIgnore::load(workspace_root);
    let Ok(paths) = operator.walk_workspace_files_ignoring(workspace_root, &ignore) else {
        return Vec::new();
    };

    let mut chunks = Vec::new();
    for path in paths {
        let rel = workspace_relative(&path, workspace_root);
        if exclude.iter().any(|ex| rel.starts_with(ex.as_str())) {
            continue;
        }

        let Some(language) = detect_source_language(&path) else {
            continue;
        };

        let size = path
            .metadata()
            .map(|metadata| metadata.len() as usize)
            .unwrap_or(0);
        if size > max_file_size {
            continue;
        }

        if let Ok(source) = fs::read_to_string(&path) {
            parse_source_file(&rel, &source, language, &mut chunks);
        }
    }

    chunks
}

pub fn update_index(index: &mut Vec<IndexChunk>, changed_path: &Path, workspace_root: &Path) {
    update_index_filtered(index, changed_path, workspace_root, &[], usize::MAX);
}

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

    let ignore = WorkspaceIgnore::load(workspace_root);
    if ignore.is_ignored(&rel, changed_path.is_dir()) {
        return;
    }

    let Some(language) = detect_source_language(changed_path) else {
        return;
    };

    let size = changed_path
        .metadata()
        .map(|m| m.len() as usize)
        .unwrap_or(0);
    if size > max_file_size {
        return;
    }

    if let Ok(source) = fs::read_to_string(changed_path) {
        parse_source_file(&rel, &source, language, index);
    }
}

fn workspace_relative(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn detect_source_language(path: &Path) -> Option<SourceLanguage> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some(SourceLanguage::Rust),
        "py" => Some(SourceLanguage::Python),
        "ts" => Some(SourceLanguage::TypeScript),
        "tsx" => Some(SourceLanguage::Tsx),
        "js" | "mjs" | "cjs" | "jsx" => Some(SourceLanguage::JavaScript),
        _ => None,
    }
}

fn parse_source_file(
    rel_path: &str,
    source: &str,
    language: SourceLanguage,
    chunks: &mut Vec<IndexChunk>,
) {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language.parser_language()).is_err() {
        return;
    }
    let tree = match parser.parse(source, None) {
        Some(tree) => tree,
        None => return,
    };

    let source_bytes = source.as_bytes();
    match language {
        SourceLanguage::Rust => {
            extract_rust_items(tree.root_node(), source_bytes, rel_path, None, chunks)
        }
        SourceLanguage::Python => {
            extract_python_items(tree.root_node(), source_bytes, rel_path, None, chunks)
        }
        SourceLanguage::TypeScript | SourceLanguage::Tsx | SourceLanguage::JavaScript => {
            extract_javascript_items(tree.root_node(), source_bytes, rel_path, None, chunks)
        }
    }
}

fn extract_rust_items(
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
            let scope_name =
                if kind == ItemKind::Impl || kind == ItemKind::Trait || kind == ItemKind::Module {
                    Some(name.clone())
                } else {
                    parent_scope.map(String::from)
                };

            push_chunk(
                chunks,
                rel_path,
                child,
                source,
                kind.clone(),
                name,
                scope_name.clone(),
            );

            if kind == ItemKind::Impl || kind == ItemKind::Trait || kind == ItemKind::Module {
                let scope = scope_name.as_deref();
                extract_rust_items(child, source, rel_path, scope, chunks);
            }
        } else if kind_str == "declaration_list" {
            extract_rust_items(child, source, rel_path, parent_scope, chunks);
        }
    }
}

fn extract_python_items(
    node: tree_sitter::Node,
    source: &[u8],
    rel_path: &str,
    parent_scope: Option<&str>,
    chunks: &mut Vec<IndexChunk>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "class_definition" => {
                let name = extract_name(child, source, &ItemKind::Class);
                push_chunk(
                    chunks,
                    rel_path,
                    child,
                    source,
                    ItemKind::Class,
                    name.clone(),
                    parent_scope.map(String::from),
                );
                extract_python_items(child, source, rel_path, Some(&name), chunks);
            }
            "function_definition" => {
                let name = extract_name(child, source, &ItemKind::Function);
                push_chunk(
                    chunks,
                    rel_path,
                    child,
                    source,
                    ItemKind::Function,
                    name,
                    parent_scope.map(String::from),
                );
            }
            _ => extract_python_items(child, source, rel_path, parent_scope, chunks),
        }
    }
}

fn extract_javascript_items(
    node: tree_sitter::Node,
    source: &[u8],
    rel_path: &str,
    parent_scope: Option<&str>,
    chunks: &mut Vec<IndexChunk>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "class_declaration" => {
                let name = extract_name(child, source, &ItemKind::Class);
                push_chunk(
                    chunks,
                    rel_path,
                    child,
                    source,
                    ItemKind::Class,
                    name.clone(),
                    parent_scope.map(String::from),
                );
                extract_javascript_items(child, source, rel_path, Some(&name), chunks);
            }
            "function_declaration" | "generator_function_declaration" | "method_definition" => {
                let name = extract_name(child, source, &ItemKind::Function);
                push_chunk(
                    chunks,
                    rel_path,
                    child,
                    source,
                    ItemKind::Function,
                    name,
                    parent_scope.map(String::from),
                );
            }
            "interface_declaration" => {
                let name = extract_name(child, source, &ItemKind::Interface);
                push_chunk(
                    chunks,
                    rel_path,
                    child,
                    source,
                    ItemKind::Interface,
                    name,
                    parent_scope.map(String::from),
                );
            }
            "type_alias_declaration" => {
                let name = extract_name(child, source, &ItemKind::TypeAlias);
                push_chunk(
                    chunks,
                    rel_path,
                    child,
                    source,
                    ItemKind::TypeAlias,
                    name,
                    parent_scope.map(String::from),
                );
            }
            "enum_declaration" => {
                let name = extract_name(child, source, &ItemKind::Enum);
                push_chunk(
                    chunks,
                    rel_path,
                    child,
                    source,
                    ItemKind::Enum,
                    name,
                    parent_scope.map(String::from),
                );
            }
            "variable_declarator" if function_valued_variable(child) => {
                let name = extract_name(child, source, &ItemKind::Function);
                push_chunk(
                    chunks,
                    rel_path,
                    child,
                    source,
                    ItemKind::Function,
                    name,
                    parent_scope.map(String::from),
                );
            }
            _ => extract_javascript_items(child, source, rel_path, parent_scope, chunks),
        }
    }
}

fn function_valued_variable(node: tree_sitter::Node) -> bool {
    node.child_by_field_name("value").is_some_and(|value| {
        matches!(
            value.kind(),
            "arrow_function" | "function" | "function_expression"
        )
    })
}

fn push_chunk(
    chunks: &mut Vec<IndexChunk>,
    rel_path: &str,
    node: tree_sitter::Node,
    source: &[u8],
    kind: ItemKind,
    name: String,
    parent_scope: Option<String>,
) {
    chunks.push(IndexChunk {
        path: rel_path.to_string(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        kind,
        name,
        parent_scope,
        source: node.utf8_text(source).unwrap_or("").to_string(),
    });
}

fn extract_name(node: tree_sitter::Node, source: &[u8], kind: &ItemKind) -> String {
    if *kind == ItemKind::Impl {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_identifier" || child.kind() == "generic_type" {
                return child.utf8_text(source).unwrap_or("_").to_string();
            }
        }
        return "_impl".to_string();
    }

    if let Some(name_node) = node.child_by_field_name("name") {
        return name_node.utf8_text(source).unwrap_or("_").to_string();
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "identifier" | "type_identifier" | "property_identifier"
        ) {
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
        parse_source_file("test.rs", source, SourceLanguage::Rust, &mut chunks);
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].kind, ItemKind::Function);
        assert_eq!(chunks[0].name, "hello_world");
        assert_eq!(chunks[0].start_line, 1);
    }

    #[test]
    fn test_parse_rust_file_extracts_struct() {
        let source = "pub struct Foo {\n    bar: i32,\n}\n";
        let mut chunks = Vec::new();
        parse_source_file("test.rs", source, SourceLanguage::Rust, &mut chunks);
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].kind, ItemKind::Struct);
        assert_eq!(chunks[0].name, "Foo");
    }

    #[test]
    fn test_parse_rust_file_extracts_impl_methods() {
        let source = "struct Foo;\nimpl Foo {\n    fn bar(&self) {}\n    fn baz(&self) {}\n}\n";
        let mut chunks = Vec::new();
        parse_source_file("test.rs", source, SourceLanguage::Rust, &mut chunks);
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Foo"));
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
    fn test_build_index_filtered_indexes_supported_languages_outside_src() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let scripts = tmp.path().join("scripts");
        let web = tmp.path().join("web");
        fs::create_dir_all(&scripts).expect("mkdir scripts");
        fs::create_dir_all(&web).expect("mkdir web");

        fs::write(
            scripts.join("worker.py"),
            "class Worker:\n    def run(self):\n        return 1\n",
        )
        .expect("write worker.py");
        fs::write(
            web.join("app.ts"),
            "interface Props {}\nclass App {}\nfunction render() {}\nconst boot = () => {};\n",
        )
        .expect("write app.ts");

        let mut python_chunks = Vec::new();
        parse_source_file(
            "scripts/worker.py",
            "class Worker:\n    def run(self):\n        return 1\n",
            SourceLanguage::Python,
            &mut python_chunks,
        );
        assert!(
            !python_chunks.is_empty(),
            "direct Python parse produced no chunks"
        );

        let chunks = build_index_filtered(tmp.path(), &[], usize::MAX);
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        let paths: Vec<&str> = chunks.iter().map(|c| c.path.as_str()).collect();

        assert!(
            paths.contains(&"scripts/worker.py"),
            "expected Python chunks, got paths={paths:?} names={names:?}"
        );
        assert!(
            paths.contains(&"web/app.ts"),
            "expected TypeScript chunks, got paths={paths:?} names={names:?}"
        );
        assert!(names.contains(&"Worker"), "got names={names:?}");
        assert!(names.contains(&"run"), "got names={names:?}");
        assert!(names.contains(&"Props"), "got names={names:?}");
        assert!(names.contains(&"App"), "got names={names:?}");
        assert!(names.contains(&"render"), "got names={names:?}");
        assert!(names.contains(&"boot"), "got names={names:?}");
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
        parse_source_file("test.rs", source, SourceLanguage::Rust, &mut chunks);
        let inner = chunks.iter().find(|c| c.name == "inner");
        assert!(inner.is_some(), "expected to find function 'inner'");
        assert_eq!(
            inner.unwrap().parent_scope.as_deref(),
            Some("mymod"),
            "function inside mod should have parent_scope = module name"
        );
    }

    #[test]
    fn test_search_config_respects_exclude_paths() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("mkdir src");
        fs::write(src.join("lib.rs"), "pub fn included_fn() {}\n").expect("write lib.rs");

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

    #[test]
    fn exclude_prefix_requires_path_boundary() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let data = tmp.path().join("src").join("data");
        let data_backup = tmp.path().join("src").join("data_backup");
        fs::create_dir_all(&data).expect("mkdir data");
        fs::create_dir_all(&data_backup).expect("mkdir data_backup");
        fs::write(data.join("lib.rs"), "pub fn in_data() {}\n").expect("write");
        fs::write(data_backup.join("lib.rs"), "pub fn in_data_backup() {}\n").expect("write");

        let chunks = build_index_filtered(tmp.path(), &["src/data/".to_string()], usize::MAX);
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(!names.contains(&"in_data"), "src/data/ must be excluded");
        assert!(
            names.contains(&"in_data_backup"),
            "src/data_backup/ must NOT be excluded by src/data/ prefix"
        );
    }

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

    #[test]
    fn test_build_index_filtered_skips_oversized_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).expect("mkdir");

        fs::write(src.join("small.rs"), "fn small_fn() {}\n").expect("write small.rs");

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
