use std::fs;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return files;
    }
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_rs_files(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    files
}

fn extract_crate_imports(path: &Path) -> Vec<(usize, String)> {
    let content = fs::read_to_string(path).expect("read file");
    let mut imports = Vec::new();
    let mut pending: Option<(usize, String)> = None;

    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.starts_with("//") {
            continue;
        }

        if let Some((_, current)) = pending.as_mut() {
            if !trimmed.is_empty() {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(trimmed);
            }
            if trimmed.contains(';') {
                let (start_line, import) = pending.take().expect("pending import");
                imports.push((
                    start_line,
                    normalize_import_for_boundary_scan(path, &import),
                ));
            }
            continue;
        }

        if starts_with_tracked_import(trimmed) {
            let start_line = index + 1;
            if trimmed.contains(';') {
                imports.push((
                    start_line,
                    normalize_import_for_boundary_scan(path, trimmed),
                ));
            } else {
                pending = Some((start_line, trimmed.to_string()));
            }
        }
    }

    if let Some((start_line, import)) = pending {
        imports.push((
            start_line,
            normalize_import_for_boundary_scan(path, &import),
        ));
    }

    imports
}

fn starts_with_tracked_import(trimmed: &str) -> bool {
    trimmed.starts_with("use crate::")
        || trimmed.starts_with("pub use crate::")
        || trimmed.starts_with("crate::")
        || trimmed.starts_with("use super::")
        || trimmed.starts_with("pub use super::")
        || trimmed.starts_with("super::")
}

fn normalize_import_for_boundary_scan(path: &Path, import: &str) -> String {
    normalize_relative_import(path, import).unwrap_or_else(|| import.to_string())
}

fn normalize_relative_import(path: &Path, import: &str) -> Option<String> {
    for prefix in ["pub use ", "use ", ""] {
        let Some(relative) = import.strip_prefix(prefix) else {
            continue;
        };
        if !relative.starts_with("super::") {
            continue;
        }

        let has_semicolon = relative.trim_end().ends_with(';');
        let mut remainder = relative.trim().trim_end_matches(';');
        let mut module_path = module_path_components(path);

        while let Some(next) = remainder.strip_prefix("super::") {
            if module_path.pop().is_none() {
                remainder = next;
                break;
            }
            remainder = next;
        }

        let mut resolved = String::from(prefix);
        resolved.push_str("crate::");
        if !module_path.is_empty() {
            resolved.push_str(&module_path.join("::"));
            resolved.push_str("::");
        }
        resolved.push_str(remainder);
        if has_semicolon {
            resolved.push(';');
        }
        return Some(resolved);
    }

    None
}

fn module_path_components(path: &Path) -> Vec<String> {
    let parts = path
        .iter()
        .map(|part| part.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let start = parts
        .iter()
        .rposition(|part| part == "src")
        .map(|index| index + 1)
        .unwrap_or_else(|| parts.len().saturating_sub(1));

    let mut modules = parts[start..].to_vec();
    let Some(stem) = modules
        .last()
        .and_then(|last| Path::new(last).file_stem())
        .and_then(|stem| stem.to_str())
        .map(str::to_string)
    else {
        return modules;
    };

    match stem.as_str() {
        "mod" => {
            modules.pop();
        }
        "lib" | "main" if modules.len() == 1 => {
            modules.clear();
        }
        _ => {
            let last = modules
                .last_mut()
                .expect("module path must have a final segment");
            *last = stem.to_string();
        }
    }

    modules
}

fn extract_crate_items(import: &str) -> Vec<String> {
    let Some((_, remainder)) = import.split_once("crate::") else {
        return Vec::new();
    };
    let remainder = remainder.trim().trim_end_matches(';');
    if let Some(grouped) = remainder.strip_prefix('{') {
        split_top_level_items(grouped)
    } else {
        vec![remainder.to_string()]
    }
}

fn split_top_level_items(grouped: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut item_start = 0usize;

    for (index, ch) in grouped.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    let item = grouped[item_start..index].trim();
                    if !item.is_empty() {
                        items.push(item.to_string());
                    }
                    return items;
                }
                depth -= 1;
            }
            ',' if depth == 0 => {
                let item = grouped[item_start..index].trim();
                if !item.is_empty() {
                    items.push(item.to_string());
                }
                item_start = index + 1;
            }
            _ => {}
        }
    }

    let item = grouped[item_start..].trim().trim_end_matches('}').trim();
    if !item.is_empty() {
        items.push(item.to_string());
    }

    items
}

fn root_module(item: &str) -> Option<&str> {
    let trimmed = item.trim();
    let end = trimmed
        .char_indices()
        .find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_'))
        .map(|(index, _)| index)
        .unwrap_or(trimmed.len());
    if end == 0 {
        None
    } else {
        Some(&trimmed[..end])
    }
}

fn import_mentions_forbidden_module(import: &str, forbidden: &str) -> bool {
    extract_crate_items(import)
        .into_iter()
        .any(|item| root_module(&item) == Some(forbidden))
}

fn runtime_item_is_allowed(item: &str) -> bool {
    let trimmed = item.trim();
    let Some(runtime_path) = trimmed.strip_prefix("runtime::") else {
        return false;
    };
    if runtime_path.starts_with("json_handoff") {
        return true;
    }
    if let Some(grouped) = runtime_path.strip_prefix('{') {
        return split_top_level_items(grouped)
            .into_iter()
            .all(|inner| inner.trim().starts_with("json_handoff"));
    }
    false
}

fn assert_no_forbidden_imports(
    dir: &Path,
    forbidden_modules: &[&str],
    layer_name: &str,
) -> Vec<String> {
    let mut violations = Vec::new();
    for file in collect_rs_files(dir) {
        for (line_no, import) in extract_crate_imports(&file) {
            for forbidden in forbidden_modules {
                if import_mentions_forbidden_module(&import, forbidden) {
                    let rel = file
                        .strip_prefix(dir.parent().unwrap_or(dir))
                        .unwrap_or(&file);
                    violations.push(format!(
                        "{layer_name} violation: {}:{line_no} imports `{forbidden}` — {import}",
                        rel.display()
                    ));
                }
            }
        }
    }
    violations
}

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn grouped_import_detection_catches_transport_modules() {
    let import = "use crate::{app::facade, server::handlers::{delegate_handler}, bin::vex};";
    assert!(import_mentions_forbidden_module(import, "server"));
    assert!(import_mentions_forbidden_module(import, "bin"));
}

#[test]
fn grouped_import_detection_uses_module_boundaries() {
    let import = "use crate::{app::facade, server_tool::helpers};";
    assert!(!import_mentions_forbidden_module(import, "server"));
}

#[test]
fn multiline_grouped_imports_are_concatenated_before_scanning() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("sample.rs");
    fs::write(
        &file,
        "use crate::{\n    app::facade,\n    server::handlers::{delegate_handler},\n};\n",
    )
    .unwrap();

    let imports = extract_crate_imports(&file);
    assert_eq!(imports.len(), 1);
    assert!(import_mentions_forbidden_module(&imports[0].1, "server"));
}

#[test]
fn relative_super_import_detection_resolves_to_crate_root_modules() {
    let temp = tempfile::tempdir().unwrap();
    let src_dir = temp.path().join("src/app");
    fs::create_dir_all(&src_dir).unwrap();
    let file = src_dir.join("sample.rs");
    fs::write(
        &file,
        "use super::super::server::{handlers::delegate_handler};\n",
    )
    .unwrap();

    let imports = extract_crate_imports(&file);
    assert_eq!(imports.len(), 1);
    assert_eq!(
        imports[0].1,
        "use crate::server::{handlers::delegate_handler};"
    );
    assert!(import_mentions_forbidden_module(&imports[0].1, "server"));
}

#[test]
fn runtime_must_not_import_cli_transport_terminal_or_tui() {
    let runtime_dir = src_dir().join("runtime");

    let forbidden = &[
        "local_api",
        "server",
        "bin",
        "tui_frontend",
        "terminal",
        "app",
        "ui",
    ];
    let violations = assert_no_forbidden_imports(&runtime_dir, forbidden, "runtime");
    assert!(
        violations.is_empty(),
        "ADR-028: runtime layer has forbidden imports:\n{}",
        violations.join("\n")
    );
}

#[test]
fn state_must_not_import_cli_transport_terminal_or_tui() {
    let state_dir = src_dir().join("state");
    let forbidden = &[
        "local_api",
        "server",
        "bin",
        "tui_frontend",
        "terminal",
        "app",
        "ui",
    ];
    let violations = assert_no_forbidden_imports(&state_dir, forbidden, "state");
    assert!(
        violations.is_empty(),
        "ADR-028: state layer has forbidden imports:\n{}",
        violations.join("\n")
    );
}

#[test]
fn api_must_not_import_cli_transport_terminal_or_tui() {
    let api_dir = src_dir().join("api");
    let forbidden = &[
        "local_api",
        "server",
        "bin",
        "tui_frontend",
        "terminal",
        "app",
        "ui",
    ];
    let violations = assert_no_forbidden_imports(&api_dir, forbidden, "api");
    assert!(
        violations.is_empty(),
        "ADR-028: api layer has forbidden imports:\n{}",
        violations.join("\n")
    );
}

#[test]
fn tools_must_not_import_cli_transport_terminal_or_tui() {
    let tools_dir = src_dir().join("tools");
    let forbidden = &[
        "local_api",
        "server",
        "bin",
        "tui_frontend",
        "terminal",
        "app",
        "ui",
    ];
    let violations = assert_no_forbidden_imports(&tools_dir, forbidden, "tools");
    assert!(
        violations.is_empty(),
        "ADR-028: tools layer has forbidden imports:\n{}",
        violations.join("\n")
    );
}

#[test]
fn app_facade_must_not_import_cli_binary() {
    let app_dir = src_dir().join("app");
    let forbidden = &["bin", "server"];
    let violations = assert_no_forbidden_imports(&app_dir, forbidden, "app");
    assert!(
        violations.is_empty(),
        "ADR-028: app facade has forbidden imports:\n{}",
        violations.join("\n")
    );
}

#[test]
fn server_uses_facade_entrypoint() {
    let server_dir = src_dir().join("server");
    assert!(
        server_dir.is_dir(),
        "ADR-028: src/server/ must exist as the transport layer"
    );
    let files = collect_rs_files(&server_dir);
    let uses_facade = files.iter().any(|file| {
        extract_crate_imports(file)
            .iter()
            .flat_map(|(_, line)| extract_crate_items(line))
            .any(|item| root_module(&item) == Some("app"))
    });
    assert!(
        uses_facade,
        "ADR-028: server module must route through the application facade (crate::app)"
    );
}

#[test]
fn server_must_not_import_runtime_directly() {
    let server_dir = src_dir().join("server");
    let files = collect_rs_files(&server_dir);
    let mut violations = Vec::new();

    for file in &files {
        for (lineno, import) in extract_crate_imports(file) {
            for item in extract_crate_items(&import) {
                if root_module(&item) == Some("runtime") && !runtime_item_is_allowed(&item) {
                    violations.push(format!("  {}:{lineno}: {import}", file.display()));
                    break;
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "ADR-028: server layer must not import crate::runtime directly \
         (route through crate::app facade):\n{}",
        violations.join("\n")
    );
}

#[test]
fn facade_module_exports_required_entrypoints() {
    let facade_src =
        fs::read_to_string(src_dir().join("app").join("facade.rs")).expect("facade.rs must exist");

    let required = &[
        "build_facade_client",
        "build_facade_runtime",
        "execute_facade_runtime",
        "run_tui_session",
    ];

    for name in required {
        assert!(
            facade_src.contains(name),
            "ADR-028: facade.rs must export `{name}`"
        );
    }
}

#[test]
fn facade_error_types_exist() {
    let errors_src =
        fs::read_to_string(src_dir().join("app").join("errors.rs")).expect("errors.rs must exist");

    assert!(
        errors_src.contains("AppError"),
        "ADR-028: errors.rs must define AppError"
    );
    assert!(
        errors_src.contains("AppResult"),
        "ADR-028: errors.rs must define AppResult"
    );
}

#[test]
fn server_must_not_import_tui_terminal_or_ui() {
    let server_dir = src_dir().join("server");
    assert!(
        server_dir.is_dir(),
        "ADR-028: src/server/ must exist as the transport layer"
    );
    let forbidden = &["tui_frontend", "terminal", "ui"];
    let violations = assert_no_forbidden_imports(&server_dir, forbidden, "server");
    assert!(
        violations.is_empty(),
        "ADR-028: server layer has forbidden imports:\n{}",
        violations.join("\n")
    );
}

#[test]
fn cli_binary_must_not_import_transport_layer() {
    let vex_src = fs::read_to_string(src_dir().join("bin").join("vex.rs"))
        .expect("src/bin/vex.rs must exist");
    assert!(
        !vex_src.contains("vexapi::server"),
        "ADR-028: src/bin/vex.rs must not import vexapi::server directly — use the crate-root re-export instead"
    );
}

#[test]
fn server_module_exists() {
    let server_rs = src_dir().join("server.rs");
    assert!(
        server_rs.is_file(),
        "ADR-028: src/server.rs must exist — transport module root"
    );
    let server_dir = src_dir().join("server");
    assert!(
        server_dir.is_dir(),
        "ADR-028: src/server/ must exist — transport layer extracted from local_api.rs"
    );
    for submodule in &["http.rs", "sse.rs", "socket.rs", "handlers.rs", "util.rs"] {
        let path = server_dir.join(submodule);
        assert!(
            path.is_file() || server_dir.join(submodule.trim_end_matches(".rs")).is_dir(),
            "ADR-028: src/server/{submodule} must exist"
        );
    }
}
