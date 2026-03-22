use anyhow::{Context, Result};
use std::path::Path;

/// The kind of top-level Rust item captured in an [`IndexChunk`].
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
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemKind::Function => "function",
            ItemKind::Struct => "struct",
            ItemKind::Enum => "enum",
            ItemKind::Impl => "impl",
            ItemKind::Trait => "trait",
            ItemKind::Module => "module",
            ItemKind::Const => "const",
            ItemKind::Static => "static",
            ItemKind::TypeAlias => "type_alias",
        }
    }
}

/// A single named code item extracted from a Rust source file.
#[derive(Debug, Clone)]
pub struct IndexChunk {
    /// Workspace-relative path to the source file.
    pub path: String,
    /// First line of the item (1-based).
    pub start_line: usize,
    /// Last line of the item (1-based).
    pub end_line: usize,
    /// Kind of the Rust item.
    pub kind: ItemKind,
    /// Declared name of the item.
    pub name: String,
    /// Name of the enclosing impl/trait/mod scope, if any.
    pub parent_scope: Option<String>,
    /// Raw source text of the item.
    pub source: String,
}

/// Walk `workspace_root/src` and return chunks for every `.rs` file found.
pub fn build_index(workspace_root: &Path) -> Result<Vec<IndexChunk>> {
    let mut chunks = Vec::new();
    let src_path = workspace_root.join("src");
    if src_path.is_dir() {
        collect_from_dir(&src_path, workspace_root, &mut chunks)?;
    }
    Ok(chunks)
}

/// Re-index a single file, replacing any existing chunks for that path.
/// `changed_path` must be the full (absolute or relative) path to the file.
pub fn update_index(index: &mut Vec<IndexChunk>, changed_path: &Path) {
    let rel = changed_path.to_string_lossy().to_string();
    index.retain(|c| c.path != rel);
    if let Ok(source) = std::fs::read_to_string(changed_path) {
        let _ = parse_rust_items(&source, &rel, index);
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn collect_from_dir(dir: &Path, root: &Path, chunks: &mut Vec<IndexChunk>) -> Result<()> {
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("iterating {}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_from_dir(&path, root, chunks)?;
        } else if path.extension().map_or(false, |ext| ext == "rs") {
            if let Err(e) = collect_from_file(&path, root, chunks) {
                eprintln!("vex index: skipping {}: {e}", path.display());
            }
        }
    }
    Ok(())
}

fn collect_from_file(path: &Path, root: &Path, chunks: &mut Vec<IndexChunk>) -> Result<()> {
    let source =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let rel = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();
    parse_rust_items(&source, &rel, chunks)
}

fn parse_rust_items(source: &str, path: &str, chunks: &mut Vec<IndexChunk>) -> Result<()> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("tree-sitter language error: {e}"))?;

    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("tree-sitter failed to parse {path}"))?;

    extract_items(tree.root_node(), source.as_bytes(), path, None, chunks);
    Ok(())
}

fn extract_items(
    node: tree_sitter::Node<'_>,
    source: &[u8],
    path: &str,
    parent_scope: Option<&str>,
    chunks: &mut Vec<IndexChunk>,
) {
    let item_kind = match node.kind() {
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
        let name = node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("")
            .to_string();

        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;

        let node_source = std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
            .unwrap_or("")
            .to_string();

        // Use the item's name as the new parent scope for nested items.
        let child_scope: Option<String> = if name.is_empty() {
            parent_scope.map(str::to_string)
        } else {
            Some(name.clone())
        };

        chunks.push(IndexChunk {
            path: path.to_string(),
            start_line,
            end_line,
            kind,
            name,
            parent_scope: parent_scope.map(str::to_string),
            source: node_source,
        });

        // Recurse into the body so nested items (e.g. methods inside impl) are captured.
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                extract_items(child, source, path, child_scope.as_deref(), chunks);
            }
        }
    } else {
        // Non-matching node — recurse without changing parent scope.
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                extract_items(child, source, path, parent_scope, chunks);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_function() {
        let src = "pub fn hello() -> &'static str { \"world\" }\n";
        let mut chunks = Vec::new();
        parse_rust_items(src, "src/lib.rs", &mut chunks).expect("parse ok");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].name, "hello");
        assert!(matches!(chunks[0].kind, ItemKind::Function));
        assert_eq!(chunks[0].start_line, 1);
    }

    #[test]
    fn test_parse_struct_and_impl() {
        let src = "struct Foo { x: i32 }\nimpl Foo { fn bar(&self) {} }\n";
        let mut chunks = Vec::new();
        parse_rust_items(src, "src/foo.rs", &mut chunks).expect("parse ok");
        let names: Vec<&str> = chunks.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Foo"));
        assert!(names.contains(&"bar"));
        // `bar` should have parent scope `Foo`
        let bar = chunks.iter().find(|c| c.name == "bar").unwrap();
        assert_eq!(bar.parent_scope.as_deref(), Some("Foo"));
    }

    #[test]
    fn test_update_index_replaces_file() {
        let src1 = "fn alpha() {}\n";
        let src2 = "fn beta() {}\n";
        let mut chunks = Vec::new();
        parse_rust_items(src1, "src/x.rs", &mut chunks).expect("parse ok");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].name, "alpha");

        // Simulate update: remove old and add new manually (since update_index reads from disk)
        chunks.retain(|c| c.path != "src/x.rs");
        parse_rust_items(src2, "src/x.rs", &mut chunks).expect("parse ok");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].name, "beta");
    }
}
