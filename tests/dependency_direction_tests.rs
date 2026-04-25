use std::fs;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !dir.is_dir() { return files; }
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() { files.extend(collect_rs_files(&path)); }
        else if path.extension().and_then(|e| e.to_str()) == Some("rs") { files.push(path); }
    }
    files
}

fn extract_crate_imports(path: &Path) -> Vec<(usize, String)> {
    let content = fs::read_to_string(path).expect("read file");
    let mut imports = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") { continue; }
        if let Some((_, current)) = pending.as_mut() {
            if !trimmed.is_empty() {
                if !current.is_empty() { current.push(' '); }
                current.push_str(trimmed);
            }
            if trimmed.contains(';') {
                let (start_line, import) = pending.take().expect("pending import");
                imports.push((start_line, normalize_import_for_boundary_scan(path, &import)));
            }
            continue;
        }
        if starts_with_tracked_import(trimmed) {
            let start_line = index + 1;
            if trimmed.contains(';') { imports.push((start_line, normalize_import_for_boundary_scan(path, trimmed))); }
            else { pending = Some((start_line, trimmed.to_string())); }
        }
    }
    if let Some((start_line, import)) = pending { imports.push((start_line, normalize_import_for_boundary_scan(path, &import))); }
    imports
}

fn starts_with_tracked_import(trimmed: &str) -> bool {
    trimmed.starts_with("use crate::") || trimmed.starts_with("pub use crate::") || trimmed.starts_with("crate::")
        || trimmed.starts_with("use super::") || trimmed.starts_with("pub use super::") || trimmed.starts_with("super::")
}

fn normalize_import_for_boundary_scan(path: &Path, import: &str) -> String {
    normalize_relative_import(path, import).unwrap_or_else(|| import.to_string())
}

fn normalize_relative_import(path: &Path, import: &str) -> Option<String> {
    for prefix in ["pub use ", "use ", ""] {
        let Some(relative) = import.strip_prefix(prefix) else { continue; };
        if !relative.starts_with("super::") { continue; }
        let has_semicolon = relative.trim_end().ends_with(';');
        let mut remainder = relative.trim().trim_end_matches(';');
        let mut module_path = module_path_components(path);
        while let Some(next) = remainder.strip_prefix("super::") {
            if module_path.pop().is_none() { remainder = next; break; }
            remainder = next;
        }
        let mut resolved = String::from(prefix);
        resolved.push_str("crate::");
        if !module_path.is_empty() { resolved.push_str(&module_path.join("::")); resolved.push_str("::"); }
        resolved.push_str(remainder);
        if has_semicolon { resolved.push(';'); }
        return Some(resolved);
    }
    None
}

fn module_path_components(path: &Path) -> Vec<String> {
    let parts = path.iter().map(|part| part.to_string_lossy().into_owned()).collect::<Vec<_>>();
    let start = parts.iter().rposition(|part| part == "src").map(|index| index + 1).unwrap_or_else(|| parts.len().saturating_sub(1));
    let mut modules = parts[start..].to_vec();
    let Some(stem) = modules.last().and_then(|last| Path::new(last).file_stem()).and_then(|stem| stem.to_str()).map(str::to_string) else { return modules; };
    match stem.as_str() {
        "mod" => { modules.pop(); }
        "lib" | "main" if modules.len() == 1 => { modules.clear(); }
        _ => { *modules.last_mut().expect("module path must have a final segment") = stem.to_string(); }
    }
    modules
}

fn extract_crate_items(import: &str) -> Vec<String> {
    let Some((_, remainder)) = import.split_once("crate::") else { return Vec::new(); };
    let remainder = remainder.trim().trim_end_matches(';');
    if let Some(grouped) = remainder.strip_prefix('{') { split_top_level_items(grouped) }
    else { vec![remainder.to_string()] }
}

fn split_top_level_items(grouped: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut item_start = 0usize;
    for (index, ch) in grouped.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                if depth == 0 { let item = grouped[item_start..index].trim(); if !item.is_empty() { items.push(item.to_string()); } return items; }
                depth -= 1;
            }
            ',' if depth == 0 => { let item = grouped[item_start..index].trim(); if !item.is_empty() { items.push(item.to_string()); } item_start = index + 1; }
            _ => {}
        }
    }
    let item = grouped[item_start..].trim().trim_end_matches('}').trim();
    if !item.is_empty() { items.push(item.to_string()); }
    items
}

fn root_module(item: &str) -> Option<&str> {
    let trimmed = item.trim();
    let end = trimmed.char_indices().find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_')).map(|(index, _)| index).unwrap_or(trimmed.len());
    if end == 0 { None } else { Some(&trimmed[..end]) }
}

fn import_mentions_forbidden_module(import: &str, forbidden: &str) -> bool {
    extract_crate_items(import).into_iter().any(|item| root_module(&item) == Some(forbidden))
}

fn runtime_item_is_allowed(item: &str) -> bool {
    let trimmed = item.trim();
    let Some(runtime_path) = trimmed.strip_prefix("runtime::") else { return false; };
    if runtime_path.starts_with("json_handoff") { return true; }
    if let Some(grouped) = runtime_path.strip_prefix('{') {
        return split_top_level_items(grouped).into_iter().all(|inner| inner.trim().starts_with("json_handoff"));
    }
    false
}

fn assert_no_forbidden_imports(dir: &Path, forbidden_modules: &[&str], layer_name: &str) -> Vec<String> {
    let mut violations = Vec::new();
    for file in collect_rs_files(dir) {
        for (line_no, import) in extract_crate_imports(&file) {
            for forbidden in forbidden_modules {
                if import_mentions_forbidden_module(&import, forbidden) {
                    let rel = file.strip_prefix(dir.parent().unwrap_or(dir)).unwrap_or(&file);
                    violations.push(format!("{layer_name} violation: {}:{line_no} imports `{forbidden}` — {import}", rel.display()));
                }
            }
        }
    }
    violations
}

fn src_dir() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("src") }

#[test]
fn lower_layers_must_not_import_cli_transport_or_tui() {
    let forbidden = &["local_api", "server", "bin", "tui_frontend", "terminal", "app", "ui"];
    for (dir_name, layer) in [("runtime", "runtime"), ("state", "state"), ("api", "api"), ("tools", "tools")] {
        let violations = assert_no_forbidden_imports(&src_dir().join(dir_name), forbidden, layer);
        assert!(violations.is_empty(), "ADR-028: {layer} layer has forbidden imports:\n{}", violations.join("\n"));
    }
}

#[test]
fn server_layer_direction_and_facade_contract() {
    let server_dir = src_dir().join("server");
    assert!(server_dir.is_dir(), "ADR-028: src/server/ must exist");

    let app_violations = assert_no_forbidden_imports(&src_dir().join("app"), &["bin", "server"], "app");
    assert!(app_violations.is_empty(), "ADR-028: app facade has forbidden imports:\n{}", app_violations.join("\n"));

    let tui_violations = assert_no_forbidden_imports(&server_dir, &["tui_frontend", "terminal", "ui"], "server");
    assert!(tui_violations.is_empty(), "ADR-028: server layer has forbidden imports:\n{}", tui_violations.join("\n"));

    let files = collect_rs_files(&server_dir);
    let uses_facade = files.iter().any(|file| extract_crate_imports(file).iter().flat_map(|(_, line)| extract_crate_items(line)).any(|item| root_module(&item) == Some("app")));
    assert!(uses_facade, "ADR-028: server must route through crate::app facade");

    let mut runtime_violations = Vec::new();
    for file in &files {
        for (lineno, import) in extract_crate_imports(file) {
            for item in extract_crate_items(&import) {
                if root_module(&item) == Some("runtime") && !runtime_item_is_allowed(&item) {
                    runtime_violations.push(format!("  {}:{lineno}: {import}", file.display()));
                    break;
                }
            }
        }
    }
    assert!(runtime_violations.is_empty(), "ADR-028: server must not import crate::runtime directly:\n{}", runtime_violations.join("\n"));
}

#[test]
fn facade_exports_required_entrypoints_and_error_types() {
    let facade_src = fs::read_to_string(src_dir().join("app").join("facade.rs")).expect("facade.rs must exist");
    for name in &["build_facade_client", "build_facade_runtime", "execute_facade_runtime", "run_tui_session"] {
        assert!(facade_src.contains(name), "ADR-028: facade.rs must export `{name}`");
    }
    let errors_src = fs::read_to_string(src_dir().join("app").join("errors.rs")).expect("errors.rs must exist");
    assert!(errors_src.contains("AppError") && errors_src.contains("AppResult"), "ADR-028: errors.rs must define AppError and AppResult");
}

#[test]
fn server_module_structure_and_cli_binary_isolation() {
    let server_rs = src_dir().join("server.rs");
    assert!(server_rs.is_file(), "ADR-028: src/server.rs must exist");
    let server_dir = src_dir().join("server");
    assert!(server_dir.is_dir(), "ADR-028: src/server/ must exist");
    for submodule in &["http.rs", "sse.rs", "socket.rs", "handlers.rs", "util.rs"] {
        let path = server_dir.join(submodule);
        assert!(path.is_file() || server_dir.join(submodule.trim_end_matches(".rs")).is_dir(), "ADR-028: src/server/{submodule} must exist");
    }
    let vex_src = fs::read_to_string(src_dir().join("bin").join("vex.rs")).expect("src/bin/vex.rs must exist");
    assert!(!vex_src.contains("vexcoder::server"), "ADR-028: src/bin/vex.rs must not import vexcoder::server directly");
}
