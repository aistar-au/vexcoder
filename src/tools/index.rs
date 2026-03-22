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
pub fn build_index(workspace_root: &Path) -> Vec<IndexChunk> {
    let mut chunks = Vec::new();
    let src_dir = workspace_root.join("src");
    if !src_dir.is_dir() {
        return chunks;
    }
    collect_rs_files(&src_dir, workspace_root, &mut chunks);
    chunks
}

/// Re-index a single file: remove old chunks for that path and re-parse.
pub fn update_index(index: &mut Vec<IndexChunk>, changed_path: &Path, workspace_root: &Path) {
    let rel = workspace_relative(changed_path, workspace_root);
    index.retain(|c| c.path != rel);
    if changed_path.extension().is_some_and(|e| e == "rs") {
        if let Ok(source) = fs::read_to_string(changed_path) {
            parse_rust_file(&rel, &source, index);
        }
    }
}

fn collect_rs_files(dir: &Path, workspace_root: &Path, chunks: &mut Vec<IndexChunk>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, workspace_root, chunks);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let rel = workspace_relative(&path, workspace_root);
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

            let scope_name = if kind == ItemKind::Impl || kind == ItemKind::Trait {
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
}
