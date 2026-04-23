use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

const HOOK_FILENAME: &str = "prepare-commit-msg";
const HOOK_MARKER: &str = "# installed by vexcoder";

const HOOK_BODY: &str = r#"#!/bin/sh
# installed by vexcoder
# Appends Vex-Task-Id and Co-authored-by trailers for active tasks with recorded changes.
# Remove with: vex uninstall-hooks

set -eu

COMMIT_MSG_FILE="$1"
COMMIT_SOURCE="${2:-}"

case "$COMMIT_SOURCE" in
  merge|squash)
    exit 0
    ;;
esac

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
STATE_DIR="${VEX_STATE_DIR:-.vex/state}"

case "$STATE_DIR" in
  /*) ;;
  *) STATE_DIR="$REPO_ROOT/$STATE_DIR" ;;
esac

LATEST_TASK_FILE="$(ls -t "$STATE_DIR"/*.json 2>/dev/null | head -n 1 || true)"
[ -n "$LATEST_TASK_FILE" ] || exit 0

if ! awk '
  /"changed_files"[[:space:]]*:[[:space:]]*\[/ { in_array=1; next }
  in_array && /\]/ { exit(found ? 0 : 1) }
  in_array && /"/ { found=1; exit 0 }
  END { exit(found ? 0 : 1) }
' "$LATEST_TASK_FILE"; then
  exit 0
fi

TASK_ID="$(basename "$LATEST_TASK_FILE" .json)"

if ! grep -q '^Vex-Task-Id: ' "$COMMIT_MSG_FILE"; then
  printf "\nVex-Task-Id: %s\n" "$TASK_ID" >> "$COMMIT_MSG_FILE"
fi

if ! grep -q '^Co-authored-by: vexcoder <vexcoder@localhost>$' "$COMMIT_MSG_FILE"; then
  printf "Co-authored-by: vexcoder <vexcoder@localhost>\n" >> "$COMMIT_MSG_FILE"
fi
"#;

fn hook_path(working_dir: &Path) -> Result<PathBuf> {
    let hooks_dir = git_output(
        working_dir,
        &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
    )?;
    Ok(PathBuf::from(hooks_dir).join(HOOK_FILENAME))
}

fn git_output(working_dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(working_dir)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "git {} failed in {}{}",
            args.join(" "),
            working_dir.display(),
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn install_hooks(working_dir: &Path) -> Result<()> {
    let path = hook_path(working_dir)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if path.exists() {
        let existing = std::fs::read_to_string(&path)?;
        if existing.contains(HOOK_MARKER) {
            println!("hook already installed: {}", path.display());
            return Ok(());
        }
        bail!(
            "a prepare-commit-msg hook already exists at {}; remove it manually before running vex install-hooks",
            path.display()
        );
    }

    std::fs::write(&path, HOOK_BODY)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms)?;
    }

    println!("installed hook: {}", path.display());
    Ok(())
}

pub fn uninstall_hooks(working_dir: &Path) -> Result<()> {
    let path = hook_path(working_dir)?;
    if !path.exists() {
        println!("no hook installed at {}", path.display());
        return Ok(());
    }

    let content = std::fs::read_to_string(&path)?;
    if !content.contains(HOOK_MARKER) {
        bail!(
            "hook at {} was not installed by vexcoder; remove it manually",
            path.display()
        );
    }

    std::fs::remove_file(&path)?;
    println!("removed hook: {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::TaskState;

    fn run_git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        if !output.status.success() {
            panic!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_repo() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        run_git(temp.path(), &["init"]);
        run_git(temp.path(), &["config", "user.email", "tester@example.com"]);
        run_git(temp.path(), &["config", "user.name", "Tester"]);
        std::fs::write(temp.path().join("README.md"), "hello\n").unwrap();
        run_git(temp.path(), &["add", "README.md"]);
        run_git(temp.path(), &["commit", "-m", "initial"]);
        temp
    }

    fn write_task_state(repo: &Path, task_id: &str, changed_files: &[&str]) {
        let mut state = TaskState::new(task_id.to_string());
        state.changed_files = changed_files.iter().map(PathBuf::from).collect();
        state.save(&TaskState::state_dir_from(repo)).unwrap();
    }

    #[test]
    fn test_install_hooks_writes_executable_script() {
        let repo = init_repo();
        install_hooks(repo.path()).unwrap();
        let hook = repo.path().join(".git/hooks/prepare-commit-msg");
        assert!(hook.exists(), "hook file must be written");
        let content = std::fs::read_to_string(&hook).unwrap();
        assert!(
            content.contains(HOOK_MARKER),
            "hook must contain vexcoder marker"
        );
        assert!(
            content.contains("changed_files"),
            "hook must inspect task state"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&hook).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "hook must be executable");
        }
    }

    #[test]
    fn test_install_hooks_is_worktree_aware() {
        let repo = init_repo();
        run_git(repo.path(), &["branch", "feature"]);
        let worktree = repo.path().join("feature-worktree");
        run_git(
            repo.path(),
            &["worktree", "add", worktree.to_str().unwrap(), "feature"],
        );

        install_hooks(&worktree).unwrap();

        assert!(repo.path().join(".git/hooks/prepare-commit-msg").exists());
    }

    #[test]
    fn test_install_hooks_refuses_foreign_hook() {
        let repo = init_repo();
        let hook = repo.path().join(".git/hooks/prepare-commit-msg");
        std::fs::write(&hook, "#!/bin/sh\necho 'operator hook'\n").unwrap();
        let result = install_hooks(repo.path());
        assert!(result.is_err(), "must refuse to overwrite a foreign hook");
        let content = std::fs::read_to_string(&hook).unwrap();
        assert!(
            content.contains("operator hook"),
            "foreign hook must be untouched"
        );
    }

    #[test]
    fn test_uninstall_hooks_removes_vexcoder_hook() {
        let repo = init_repo();
        install_hooks(repo.path()).unwrap();
        uninstall_hooks(repo.path()).unwrap();
        assert!(!repo.path().join(".git/hooks/prepare-commit-msg").exists());
    }

    #[test]
    fn test_hook_appends_trailers_for_changed_files() {
        let repo = init_repo();
        install_hooks(repo.path()).unwrap();
        write_task_state(repo.path(), "task-123", &["README.md"]);

        std::fs::write(repo.path().join("README.md"), "updated\n").unwrap();
        run_git(repo.path(), &["add", "README.md"]);
        run_git(repo.path(), &["commit", "-m", "update readme"]);

        let message = run_git(repo.path(), &["log", "-1", "--pretty=%B"]);
        assert!(message.contains("Vex-Task-Id: task-123"));
        assert!(message.contains("Co-authored-by: vexcoder <vexcoder@localhost>"));
    }

    #[test]
    fn test_hook_skips_trailers_without_changed_files() {
        let repo = init_repo();
        install_hooks(repo.path()).unwrap();
        write_task_state(repo.path(), "task-123", &[]);

        std::fs::write(repo.path().join("README.md"), "updated\n").unwrap();
        run_git(repo.path(), &["add", "README.md"]);
        run_git(repo.path(), &["commit", "-m", "update readme"]);

        let message = run_git(repo.path(), &["log", "-1", "--pretty=%B"]);
        assert!(!message.contains("Vex-Task-Id:"));
        assert!(!message.contains("Co-authored-by: vexcoder <vexcoder@localhost>"));
    }
}
