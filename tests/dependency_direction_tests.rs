//! ADR-028 dependency-direction enforcement tests.
//!
//! These tests scan Rust source files to verify that the layered architecture
//! enforces inward-only dependency direction:
//!
//!   CLI (src/bin/) -> Application facade (src/app/) -> Runtime (src/runtime/)
//!   Transport (src/local_api.rs) -> Application facade (src/app/)
//!
//! Forbidden directions:
//!   - Runtime must NOT import CLI, transport, terminal, or TUI frontend
//!   - State/conversation must NOT import CLI, transport, terminal, or TUI
//!   - Application facade must NOT import CLI (src/bin/)

use std::fs;
use std::path::{Path, PathBuf};

/// Collect all `.rs` files under a directory, recursively.
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

/// Extract all `use crate::...` imports from a Rust source file.
fn extract_crate_imports(path: &Path) -> Vec<(usize, String)> {
    let content = fs::read_to_string(path).expect("read file");
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with("use crate::") || trimmed.starts_with("crate::") {
                Some((i + 1, trimmed.to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// Check that no file in `dir` imports any of the `forbidden_modules`.
fn assert_no_forbidden_imports(
    dir: &Path,
    forbidden_modules: &[&str],
    layer_name: &str,
) -> Vec<String> {
    let mut violations = Vec::new();
    for file in collect_rs_files(dir) {
        for (line_no, import) in extract_crate_imports(&file) {
            for forbidden in forbidden_modules {
                if import.contains(&format!("crate::{forbidden}"))
                    || import.contains(&format!("crate::{forbidden}::"))
                {
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

// ---------------------------------------------------------------------------
// ADR-028 Rule: Runtime must NOT import CLI, transport, terminal, or TUI
// ---------------------------------------------------------------------------

#[test]
fn runtime_must_not_import_cli_transport_terminal_or_tui() {
    let runtime_dir = src_dir().join("runtime");
    let forbidden = &["local_api", "tui_frontend", "terminal", "app", "ui"];
    let violations = assert_no_forbidden_imports(&runtime_dir, forbidden, "runtime");
    assert!(
        violations.is_empty(),
        "ADR-028: runtime layer has forbidden imports:\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// ADR-028 Rule: State/conversation must NOT import CLI, transport, or TUI
// ---------------------------------------------------------------------------

#[test]
fn state_must_not_import_cli_transport_terminal_or_tui() {
    let state_dir = src_dir().join("state");
    let forbidden = &["local_api", "tui_frontend", "terminal", "app", "ui"];
    let violations = assert_no_forbidden_imports(&state_dir, forbidden, "state");
    assert!(
        violations.is_empty(),
        "ADR-028: state layer has forbidden imports:\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// ADR-028 Rule: API client must NOT import CLI, transport, terminal, or TUI
// ---------------------------------------------------------------------------

#[test]
fn api_must_not_import_cli_transport_terminal_or_tui() {
    let api_dir = src_dir().join("api");
    let forbidden = &["local_api", "tui_frontend", "terminal", "app", "ui"];
    let violations = assert_no_forbidden_imports(&api_dir, forbidden, "api");
    assert!(
        violations.is_empty(),
        "ADR-028: api layer has forbidden imports:\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// ADR-028 Rule: Tools must NOT import CLI, transport, terminal, or TUI
// ---------------------------------------------------------------------------

#[test]
fn tools_must_not_import_cli_transport_terminal_or_tui() {
    let tools_dir = src_dir().join("tools");
    let forbidden = &["local_api", "tui_frontend", "terminal", "app", "ui"];
    let violations = assert_no_forbidden_imports(&tools_dir, forbidden, "tools");
    assert!(
        violations.is_empty(),
        "ADR-028: tools layer has forbidden imports:\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// ADR-028 Rule: Application facade must NOT import the CLI binary
// ---------------------------------------------------------------------------

#[test]
fn app_facade_must_not_import_cli_binary() {
    let app_dir = src_dir().join("app");
    // The facade may import local_api (it wraps serve_local_api), but must
    // never reach back into the CLI binary entrypoint.
    let forbidden = &["bin"];
    let violations = assert_no_forbidden_imports(&app_dir, forbidden, "app");
    assert!(
        violations.is_empty(),
        "ADR-028: app facade has forbidden imports:\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// ADR-028 Rule: Transport (local_api) reaches runtime ONLY through facade
// ---------------------------------------------------------------------------

#[test]
fn local_api_uses_facade_entrypoint() {
    let local_api = src_dir().join("local_api.rs");
    let imports = extract_crate_imports(&local_api);

    // local_api MUST import from crate::app (the facade layer)
    let uses_facade = imports.iter().any(|(_, line)| line.contains("crate::app"));
    assert!(
        uses_facade,
        "ADR-028: local_api.rs must route through the application facade (crate::app)"
    );
}

// ---------------------------------------------------------------------------
// Structural: key facade entrypoints must exist
// ---------------------------------------------------------------------------

#[test]
fn facade_module_exports_required_entrypoints() {
    let facade_src =
        fs::read_to_string(src_dir().join("app").join("facade.rs")).expect("facade.rs must exist");

    let required = &[
        "build_facade_client",
        "build_facade_runtime",
        "execute_facade_runtime",
        "run_tui_session",
        "serve_facade_local_api",
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

// ---------------------------------------------------------------------------
// Structural: server module must exist when transport is extracted
// (This test documents the next migration target — currently local_api.rs
// holds all transport code inline. When src/server/ is created, this test
// will verify it doesn't reach back into runtime.)
// ---------------------------------------------------------------------------

#[test]
fn server_module_direction_if_present() {
    let server_dir = src_dir().join("server");
    if !server_dir.is_dir() {
        // Server module not yet extracted — skip gracefully.
        eprintln!(
            "[adr-028] src/server/ does not exist yet — \
             transport code is still in src/local_api.rs"
        );
        return;
    }
    let forbidden = &["tui_frontend", "terminal", "ui"];
    let violations = assert_no_forbidden_imports(&server_dir, forbidden, "server");
    assert!(
        violations.is_empty(),
        "ADR-028: server layer has forbidden imports:\n{}",
        violations.join("\n")
    );
}
