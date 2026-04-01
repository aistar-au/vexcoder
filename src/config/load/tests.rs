use super::*;
use std::io::Write;
use tempfile::TempDir;

fn write_config(dir: &TempDir, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    let mut f = std::fs::File::create(&path).expect("create config file");
    f.write_all(content.as_bytes()).expect("write");
    path
}

/// Anchor test: the `[search]` section must be parsed from both the user and
/// repo-local config layers, with the repo layer overriding the user layer for
/// fields that are explicitly set.
#[test]
fn test_search_config_loads_from_both_layers() {
    let dir = tempfile::tempdir().expect("tempdir");

    // User config: set enabled = false and a custom exclude list.
    let user_cfg = write_config(
        &dir,
        "user.toml",
        r#"
[search]
enabled = false
exclude = ["src/vendor/"]
"#,
    );

    // Repo config lives at <cwd>/.vex/config.toml; it enables search.
    let vex_dir = dir.path().join(".vex");
    std::fs::create_dir_all(&vex_dir).expect("mkdir .vex");
    let repo_cfg = vex_dir.join("config.toml");
    std::fs::write(
        &repo_cfg,
        r#"
[search]
enabled = true
"#,
    )
    .expect("write repo config");

    let config = load_for_tests(dir.path(), Some(&user_cfg), None).expect("load_for_tests failed");

    // Repo layer enables search (overrides user layer disabled).
    assert!(
        config.search.enabled,
        "repo layer must override user layer for [search].enabled"
    );
    // User layer exclusion must survive the merge.
    assert!(
        config.search.exclude.contains(&"src/vendor/".to_string()),
        "user layer exclude list must be visible when repo layer omits it"
    );
}

#[test]
fn search_exclude_entries_are_normalized_with_trailing_slash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let user_cfg = write_config(
        &dir,
        "user.toml",
        r#"
[search]
exclude = ["src", "vendor", "build/"]
"#,
    );

    let config = load_for_tests(dir.path(), Some(&user_cfg), None).expect("load_for_tests failed");

    assert!(
        config.search.exclude.contains(&"src/".to_string()),
        "exclude entry without trailing slash must be normalized"
    );
    assert!(
        config.search.exclude.contains(&"vendor/".to_string()),
        "exclude entry without trailing slash must be normalized"
    );
    assert!(
        config.search.exclude.contains(&"build/".to_string()),
        "exclude entry with trailing slash must remain unchanged"
    );
}
