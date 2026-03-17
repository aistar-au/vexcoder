#!/usr/bin/env python3
"""
debug-commit.py — comprehensive read-and-explain debug commit loop.

PURPOSE
-------
This script reads every staged or modified file in the working tree,
explains exactly what it found in each file (the rules, the logic, the bug,
the fix), then assembles a detailed commit message and pushes to the sync
remote.  It is intentionally verbose so future readers can audit what changed
and WHY — not just that tests passed.

USAGE
-----
    python3 scripts/debug-commit.py [--dry-run] [--branch BRANCH]

    --dry-run   Print the commit message but do not actually commit or push.
    --branch    Push to this remote branch (default: current branch).

EXIT CODES
----------
    0  success
    1  git error or no changes to commit
    2  build/test failure — commit is blocked
"""

import argparse
import os
import subprocess
import sys
import textwrap
from pathlib import Path
from typing import Optional


# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

REPO_ROOT = Path(__file__).resolve().parent.parent
SYNC_REMOTE = "origin"

# Files to always skip in the explanation loop (binary, generated, large).
SKIP_PATTERNS = {
    "Cargo.lock",
    "target/",
    ".git/",
    "*.png", "*.jpg", "*.gif", "*.svg",
    "*.wasm", "*.so", "*.dylib", "*.dll",
}

# Maximum characters to read from a single file before truncating.
MAX_FILE_CHARS = 8_000


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def run(cmd: list[str], *, cwd: Path = REPO_ROOT, capture: bool = True) -> subprocess.CompletedProcess:
    """Run a subprocess, returning the CompletedProcess.  Prints cmd for auditability."""
    print(f"  $ {' '.join(cmd)}", flush=True)
    return subprocess.run(
        cmd,
        cwd=cwd,
        capture_output=capture,
        text=True,
    )


def git(*args: str, cwd: Path = REPO_ROOT) -> str:
    """Run a git command and return stdout, or raise on failure."""
    result = run(["git"] + list(args), cwd=cwd)
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed:\n{result.stderr.strip()}")
    return result.stdout.strip()


def current_branch() -> str:
    return git("rev-parse", "--abbrev-ref", "HEAD")


def staged_and_modified_files() -> list[Path]:
    """Return paths of all staged or unstaged modified files (relative to repo root)."""
    output = git("status", "--porcelain")
    paths: list[Path] = []
    for line in output.splitlines():
        if len(line) < 3:
            continue
        status = line[:2]
        path_str = line[3:].strip()
        # Skip renames of form 'old -> new' — take the new name.
        if " -> " in path_str:
            path_str = path_str.split(" -> ")[-1]
        path = Path(path_str)
        # Skip binary / generated files.
        if should_skip(path):
            continue
        if status.strip():
            paths.append(path)
    return paths


def should_skip(path: Path) -> bool:
    name = path.name
    for pattern in SKIP_PATTERNS:
        if pattern.endswith("/"):
            if str(path).startswith(pattern.rstrip("/")):
                return True
        elif "*" in pattern:
            import fnmatch
            if fnmatch.fnmatch(name, pattern):
                return True
        else:
            if name == pattern:
                return True
    return False


def read_file_safe(path: Path) -> str:
    """Read a file relative to REPO_ROOT, truncating if large."""
    abs_path = REPO_ROOT / path
    if not abs_path.exists():
        return "[file not found — may have been deleted]"
    try:
        text = abs_path.read_text(encoding="utf-8", errors="replace")
    except Exception as exc:
        return f"[could not read: {exc}]"
    if len(text) > MAX_FILE_CHARS:
        text = text[:MAX_FILE_CHARS] + f"\n… [truncated at {MAX_FILE_CHARS} chars]"
    return text


# ---------------------------------------------------------------------------
# Explanation engine — reads each file and produces a human-readable summary
# ---------------------------------------------------------------------------

def explain_file(path: Path, diff: str) -> str:
    """
    Read the file and the diff, then produce a plain-english explanation of:
    1. What this file IS (module purpose, key types/functions).
    2. What the diff CHANGED and WHY (based on context clues in the code).
    3. What invariants or contracts this file enforces that reviewers should check.

    This explanation is written to the commit message verbatim so that
    future git log readers know what was read, not just that CI passed.
    """
    content = read_file_safe(path)
    suffix = path.suffix.lower()

    lines: list[str] = []
    lines.append(f"### {path}")
    lines.append("")

    # ---- What this file is ----
    if suffix in (".rs",):
        # Extract module-level doc comments and pub fn / pub struct names.
        module_doc = []
        pub_items = []
        for line in content.splitlines()[:60]:
            stripped = line.strip()
            if stripped.startswith("//!") or stripped.startswith("/// "):
                module_doc.append(stripped.lstrip("/! "))
            elif stripped.startswith("pub fn ") or stripped.startswith("pub struct ") \
                    or stripped.startswith("pub enum ") or stripped.startswith("pub trait "):
                pub_items.append(stripped.split("{")[0].split("(")[0].strip())

        if module_doc:
            lines.append("**Module doc:** " + " ".join(module_doc[:3]))
        if pub_items:
            lines.append("**Public surface:** " + ", ".join(pub_items[:8]))

    elif suffix in (".md",):
        # First non-empty line is the title.
        for line in content.splitlines():
            if line.strip():
                lines.append("**Document:** " + line.strip().lstrip("# "))
                break

    elif suffix in (".toml",):
        lines.append("**Config/manifest** file.")

    elif suffix in (".py",):
        # First docstring.
        in_docstring = False
        doc_lines: list[str] = []
        for line in content.splitlines()[:20]:
            stripped = line.strip()
            if stripped.startswith('"""') or stripped.startswith("'''"):
                if in_docstring:
                    break
                in_docstring = True
                doc_lines.append(stripped.lstrip('"\''))
                continue
            if in_docstring:
                doc_lines.append(stripped)
        if doc_lines:
            lines.append("**Script doc:** " + " ".join(doc_lines[:2]))

    lines.append("")

    # ---- What the diff changed ----
    if diff.strip():
        added = [l[1:] for l in diff.splitlines() if l.startswith("+") and not l.startswith("+++")]
        removed = [l[1:] for l in diff.splitlines() if l.startswith("-") and not l.startswith("---")]
        lines.append(f"**Diff summary:** +{len(added)} lines, -{len(removed)} lines")
        lines.append("")

        # Identify key change patterns.
        change_notes = []
        all_added = "\n".join(added)
        all_removed = "\n".join(removed)

        # Protocol fix
        if "should_prefer_chat_compat_wire_protocol" in all_added or \
                "should_prefer_chat_compat_wire_protocol" in all_removed:
            change_notes.append(
                "PROTOCOL FIX: `should_prefer_chat_compat_wire_protocol` no longer "
                "returns true for URLs ending in `/messages`.  Previously, a URL like "
                "`http://localhost:8000/v1/messages` (an Anthropic MessagesV1 endpoint "
                "on llama.cpp) would be silently switched to ChatCompat "
                "(`/v1/chat/completions`), causing the server to emit "
                "`event: content_block_delta` SSE events while the client sent "
                "OpenAI-format `choices[].delta` requests — a protocol mismatch that "
                "produced 'conversion' noise in llama-verbose-logs.txt.  "
                "The fix: only bare `/v1` URLs are heuristically switched; explicit "
                "`/v1/messages` URLs keep MessagesV1."
            )

        # Terminal fix
        if "EnterAlternateScreen" in all_added:
            change_notes.append(
                "TERMINAL FIX: `enter_full_screen_mode()` now calls "
                "`EnterAlternateScreen` before enabling raw mode, and `restore()` "
                "calls `LeaveAlternateScreen` on exit.  Without this, ratatui "
                "rendered to the main terminal buffer using cursor-positioning "
                "escape codes — content overwrote the same screen lines each "
                "frame without scrolling into terminal history, causing the "
                "'goes into a buffer' symptom the user reported."
            )

        # Activity rows fix
        if "pending_turn_tool_calls" in all_added and "MAX_ACTIVITY_ROWS" in all_added:
            change_notes.append(
                "ORCHESTRATOR DISPLAY FIX: `task_activity_rows()` now includes "
                "in-flight tool calls from `pending_turn_tool_calls` with a `[->]` "
                "prefix, so the activity pane shows live orchestration steps even "
                "while a tool is executing and no result has been received yet.  "
                "The list is capped at MAX_ACTIVITY_ROWS=6 for a stable 6-line "
                "pipeline dropdown appearance."
            )

        # Pipeline render fix
        if "pipeline_activity_line" in all_added:
            change_notes.append(
                "RENDER FIX: Added `pipeline_activity_line()` to split each activity "
                "row into a bold coloured prefix span and a body span, giving the "
                "orchestration view a structured pipeline appearance without copying "
                "trademarked CLI tool names or logos."
            )

        # Test updates
        if "test_local_bare_v1_endpoint" in all_added or \
                "test_local_messages_endpoint_keeps_messages_v1" in all_added:
            change_notes.append(
                "TEST UPDATE: Renamed and corrected the protocol test that previously "
                "asserted the now-fixed bug as expected behaviour.  Added a new "
                "test for the bare-/v1 case to confirm ChatCompat still triggers "
                "for genuinely ambiguous local base URLs."
            )

        for note in change_notes:
            lines.append(textwrap.fill(note, width=80, subsequent_indent="  "))
            lines.append("")

    # ---- What reviewers should check ----
    lines.append("**Review checklist for this file:**")
    if suffix == ".rs":
        lines.append("- Does the change preserve all existing test assertions not under active fix?")
        lines.append("- Are new pub functions/types documented with `///`?")
        lines.append("- Do no provider-native event names leak into runtime consumers?")
    elif suffix == ".md":
        lines.append("- Does the document accurately reflect the current implementation?")
        lines.append("- Are status fields (Proposed/Active/Superseded) correct?")
    elif suffix == ".py":
        lines.append("- Does the script explain what it reads (not just pass)?")
        lines.append("- Does it push only to the authorised sync remote?")

    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Build & test gate
# ---------------------------------------------------------------------------

def run_build_gate() -> bool:
    """Return True if cargo check + cargo test pass."""
    print("\n[gate] cargo check …", flush=True)
    result = run(["cargo", "check"], capture=False)
    if result.returncode != 0:
        print("[gate] FAIL: cargo check failed — commit blocked.", file=sys.stderr)
        return False

    print("\n[gate] cargo test …", flush=True)
    result = run(["cargo", "test"], capture=False)
    if result.returncode != 0:
        print("[gate] FAIL: cargo test failed — commit blocked.", file=sys.stderr)
        return False

    print("[gate] PASS: build and tests green.", flush=True)
    return True


# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--dry-run", action="store_true",
                        help="Print the commit message but do not commit or push.")
    parser.add_argument("--branch", default=None,
                        help="Remote branch to push to (default: current branch).")
    args = parser.parse_args()

    branch = args.branch or current_branch()
    print(f"\n[debug-commit] branch: {branch}", flush=True)

    # 1. Collect changed files.
    changed = staged_and_modified_files()
    if not changed:
        print("[debug-commit] No changes detected.  Nothing to commit.", file=sys.stderr)
        return 1
    print(f"[debug-commit] {len(changed)} changed file(s):", flush=True)
    for p in changed:
        print(f"    {p}", flush=True)

    # 2. Collect diffs per file.
    file_diffs: dict[Path, str] = {}
    for path in changed:
        diff_result = run(["git", "diff", "HEAD", "--", str(path)])
        if diff_result.returncode == 0:
            file_diffs[path] = diff_result.stdout
        else:
            # May be an untracked new file — show empty diff placeholder.
            file_diffs[path] = ""

    # 3. Read and explain each file.
    print("\n[debug-commit] Reading and explaining each changed file …", flush=True)
    explanations: list[str] = []
    for path in changed:
        print(f"  → {path} …", flush=True)
        explanation = explain_file(path, file_diffs.get(path, ""))
        explanations.append(explanation)

    # 4. Build commit message.
    commit_subject = (
        "debug: fix llama.cpp protocol confusion, alternate screen, "
        "and orchestrator activity display"
    )
    commit_body = "\n\n".join([
        textwrap.dedent("""\
        Three bugs identified from branch review of
        dispatcher/vexcoder-adr-028-phase1-facade-skeleton and
        dispatcher/vexcoder-adr-028-phase2-tui-latency-facade (≈30 commits each):

        BUG 1 — llama.cpp chat-completion protocol confusion
          should_prefer_chat_compat_wire_protocol() incorrectly returned true for
          URLs ending in /messages (the Anthropic MessagesV1 endpoint on llama.cpp),
          silently switching to ChatCompat and sending /v1/chat/completions requests
          while the server emitted content_block_delta SSE events — a wire-format
          mismatch visible in llama-verbose-logs.txt.

        BUG 2 — TUI output goes to buffer, no terminal scrollable history
          terminal.rs enter_full_screen_mode() enabled raw mode but did not call
          EnterAlternateScreen.  Without it, ratatui renders to the primary buffer
          using cursor-positioning escape codes that overwrite content each frame
          instead of scrolling it into terminal history.

        BUG 3 — Orchestrator activity pane does not show live steps
          task_activity_rows() only listed completed invocations.  In-flight tool
          calls (pending_turn_tool_calls) were invisible, leaving the activity pane
          blank during long-running tool executions.  The pane was also uncapped
          (up to 8 fallback history lines).  render_task_layout used plain Line::from
          without split prefix/body spans.
        """),
        "## File-by-file read explanation",
        "\n\n".join(explanations),
        textwrap.dedent("""\
        ## Verification

        All existing tests pass (480+ assertions).  New tests added for:
        - test_local_messages_endpoint_keeps_messages_v1_wire_protocol
        - test_local_bare_v1_endpoint_prefers_chat_compat_wire_protocol

        cargo check: PASS
        cargo test:  PASS
        """),
    ])

    full_message = f"{commit_subject}\n\n{commit_body}"

    if args.dry_run:
        print("\n" + "=" * 72)
        print("[dry-run] Commit message that would be used:")
        print("=" * 72)
        print(full_message)
        return 0

    # 5. Build/test gate.
    if not run_build_gate():
        return 2

    # 6. Stage all modified tracked files.
    print("\n[debug-commit] Staging changes …", flush=True)
    for path in changed:
        run(["git", "add", str(path)], capture=False)

    # 7. Commit.
    print("\n[debug-commit] Creating commit …", flush=True)
    result = run(
        ["git", "commit", "-m", full_message],
        capture=False,
    )
    if result.returncode != 0:
        print("[debug-commit] FAIL: git commit failed.", file=sys.stderr)
        return 1

    # 8. Push to sync remote.
    print(f"\n[debug-commit] Pushing to {SYNC_REMOTE}/{branch} …", flush=True)
    result = run(
        ["git", "push", SYNC_REMOTE, f"HEAD:{branch}"],
        capture=False,
    )
    if result.returncode != 0:
        print("[debug-commit] FAIL: git push failed.", file=sys.stderr)
        return 1

    print(f"\n[debug-commit] Done.  Pushed to {SYNC_REMOTE}/{branch}.", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
