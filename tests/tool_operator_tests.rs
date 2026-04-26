use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use vexcoder::tools::ToolOperator;

#[test]
fn path_traversal_and_symlink_escape_are_blocked() {
    let workspace = TempDir::new().unwrap();
    let executor = ToolOperator::new(workspace.path().to_path_buf());

    assert!(executor.read_file("../../etc/passwd").is_err());
    assert!(executor.read_file("/etc/passwd").is_err());
    assert!(executor.list_files(Some("../"), 10).is_err());

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.txt"), "secret\n").unwrap();
        symlink(
            outside.path().join("secret.txt"),
            workspace.path().join("link.txt"),
        )
        .unwrap();
        assert!(executor.read_file("link.txt").is_err());
        assert!(executor.edit_file("link.txt", "secret", "public").is_err());
        symlink(outside.path(), workspace.path().join("out")).unwrap();
        assert!(executor.write_file("out/pwn.txt", "x").is_err());
    }
}

#[test]
fn file_crud_write_read_range_rename_list_and_search() {
    let temp = TempDir::new().unwrap();
    let executor = ToolOperator::new(temp.path().to_path_buf());

    executor
        .write_file("src/cal.rs", "fn radical(n: f64) -> f64 { n.sqrt() }\n")
        .unwrap();
    assert!(
        executor
            .read_file("src/cal.rs")
            .unwrap()
            .contains("radical")
    );

    executor.write_file("my..file.txt", "content").unwrap();
    assert_eq!(executor.read_file("my..file.txt").unwrap(), "content");

    executor
        .write_file("lines.txt", "one\ntwo\nthree\n")
        .unwrap();
    assert_eq!(
        executor
            .read_file_range("lines.txt", Some(5), None)
            .unwrap(),
        "(file has 3 lines, offset 5 is past end)"
    );

    let listed = executor.list_files(Some("."), 100).unwrap();
    assert!(listed.contains("src/") && listed.contains("lines.txt"));

    let searched = executor.search_files("radical", Some("."), 20).unwrap();
    assert!(searched.contains("src/cal.rs:1"));

    let case_insensitive = executor.search_files("literal", Some("."), 20).unwrap();
    let case_sensitive = executor.search_files("LITERAL", Some("."), 20).unwrap();
    assert_eq!(case_sensitive, "No matches found.");
    drop(case_insensitive);

    let rename_result = executor
        .rename_file("src/cal.rs", "src/renamed.rs")
        .unwrap();
    assert!(rename_result.contains("Renamed"));
    assert!(!temp.path().join("src/cal.rs").exists());
    assert!(temp.path().join("src/renamed.rs").exists());
}

#[test]
fn edit_file_rejects_ambiguous_whole_file_and_oversized_snippets() {
    let temp = TempDir::new().unwrap();
    let executor = ToolOperator::new(temp.path().to_path_buf());

    executor.write_file("test.txt", "foo\nfoo\n").unwrap();
    let err = executor.edit_file("test.txt", "foo", "bar").unwrap_err();
    assert!(err.to_string().contains("appears 2 times"));

    let original = "line one\nline two\n";
    executor.write_file("whole.txt", original).unwrap();
    let err2 = executor
        .edit_file("whole.txt", original, "replaced\n")
        .unwrap_err();
    assert!(err2.to_string().contains("refuses full-file replacement"));

    let big: String = (0..120).map(|i| format!("line {i}\n")).collect();
    executor.write_file("big.txt", &big).unwrap();
    let err3 = executor
        .edit_file("big.txt", &big, "replacement\n")
        .unwrap_err();
    assert!(err3.to_string().contains("requires focused snippets"));
}

#[test]
fn empty_and_whitespace_paths_are_rejected() {
    let temp = TempDir::new().unwrap();
    let executor = ToolOperator::new(temp.path().to_path_buf());

    for variant in &["", " ", "\t"] {
        assert!(
            executor
                .read_file(variant)
                .unwrap_err()
                .to_string()
                .contains("Path cannot be empty"),
            "variant: {variant:?}"
        );
    }
    assert!(executor.write_file("", "content").is_err());
    assert!(executor.rename_file("", "target.txt").is_err());
    assert!(executor.rename_file("existing.txt", "").is_err());
}

#[test]
fn git_status_diff_add_commit_log_and_show() {
    let temp = TempDir::new().unwrap();
    init_git_repo(temp.path());
    let executor = ToolOperator::new(temp.path().to_path_buf());

    fs::write(temp.path().join("note.txt"), "line one\n").unwrap();
    run_git(temp.path(), &["add", "--", "note.txt"]);
    run_git(
        temp.path(),
        &["commit", "-m", "initial commit", "--no-gpg-sign"],
    );
    fs::write(temp.path().join("note.txt"), "line one\nline two\n").unwrap();

    assert!(
        executor
            .git_status(true, None)
            .unwrap()
            .contains("note.txt")
    );
    assert!(
        executor
            .git_diff(false, Some("note.txt"))
            .unwrap()
            .contains("+line two")
    );
    assert!(executor.git_add("note.txt").unwrap().contains("Staged"));
    assert!(
        executor
            .git_diff(true, Some("note.txt"))
            .unwrap()
            .contains("+line two")
    );
    assert!(
        executor
            .git_commit("update note")
            .unwrap()
            .contains("update note")
    );
    assert!(executor.git_log(5).unwrap().contains("update note"));
    assert!(executor.git_show("HEAD").unwrap().contains("update note"));
}

fn init_git_repo(path: &Path) {
    run_git(path, &["init"]);
    run_git(path, &["config", "user.email", "vexcoder@example.com"]);
    run_git(path, &["config", "user.name", "vexcoder test"]);
}

fn run_git(path: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}
