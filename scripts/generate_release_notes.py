#!/usr/bin/env python3

from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path

SEMVER_TAG_RE = re.compile(
    r"^v\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)

SECTION_ORDER = [
    "Features",
    "Fixes",
    "Documentation",
    "Refactors",
    "Tests",
    "CI and build",
    "Other changes",
]

SECTION_RULES = [
    (re.compile(r"^feat(?:\([^)]+\))?!?:\s*(.+)$", re.IGNORECASE), "Features"),
    (re.compile(r"^(?:fix|perf)(?:\([^)]+\))?!?:\s*(.+)$", re.IGNORECASE), "Fixes"),
    (re.compile(r"^docs(?:\([^)]+\))?!?:\s*(.+)$", re.IGNORECASE), "Documentation"),
    (re.compile(r"^refactor(?:\([^)]+\))?!?:\s*(.+)$", re.IGNORECASE), "Refactors"),
    (re.compile(r"^test(?:\([^)]+\))?!?:\s*(.+)$", re.IGNORECASE), "Tests"),
    (re.compile(r"^(?:build|ci|chore)(?:\([^)]+\))?!?:\s*(.+)$", re.IGNORECASE), "CI and build"),
]


def run_git(*args: str) -> str:
    result = subprocess.run(
        ["git", *args],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def get_repo_slug() -> str:
    if os.getenv("GITHUB_REPOSITORY"):
        return os.environ["GITHUB_REPOSITORY"]

    remote_url = run_git("config", "--get", "remote.origin.url")
    if remote_url.endswith(".git"):
        remote_url = remote_url[:-4]
    if remote_url.startswith("git@github.com:"):
        return remote_url.split(":", 1)[1]
    if remote_url.startswith("https://github.com/"):
        return remote_url.split("https://github.com/", 1)[1]
    raise SystemExit(f"FAIL: could not derive repository slug from remote.origin.url: {remote_url}")


def get_previous_tag(current_tag: str) -> str | None:
    tags = run_git("tag", "--list", "v*", "--sort=-v:refname")
    for candidate in tags.splitlines():
        candidate = candidate.strip()
        if candidate and candidate != current_tag and SEMVER_TAG_RE.fullmatch(candidate):
            return candidate
    return None


def get_release_date(tag: str) -> str:
    return run_git("log", "-1", "--format=%cs", f"{tag}^{{commit}}")


def get_commit_entries(range_spec: str) -> list[tuple[str, str]]:
    output = run_git("log", "--no-merges", "--format=%s%x1f%h", range_spec)
    if not output:
        return []

    entries: list[tuple[str, str]] = []
    for line in output.splitlines():
        subject, short_sha = line.split("\x1f", 1)
        entries.append((subject.strip(), short_sha.strip()))
    return entries


def classify_subject(subject: str) -> tuple[str, str]:
    cleaned_subject = subject.strip()
    for pattern, section in SECTION_RULES:
        match = pattern.match(cleaned_subject)
        if match:
            return section, match.group(1).strip()
    return "Other changes", cleaned_subject


def render_release_notes(tag: str, repo_slug: str, previous_tag: str | None, entries: list[tuple[str, str]]) -> str:
    release_date = get_release_date(tag)
    sections = {name: [] for name in SECTION_ORDER}
    for subject, short_sha in entries:
        section, cleaned_subject = classify_subject(subject)
        sections[section].append(f"- {cleaned_subject} ({short_sha})")

    lines = [
        f"# {tag}",
        "",
        "Generated from the commit history captured by this tag.",
        "",
        f"- Release date: {release_date}",
        f"- Semver channel: {'pre-release' if '-' in tag else 'stable'}",
    ]

    if previous_tag:
        lines.append(f"- Previous tag: {previous_tag}")
        lines.append(
            f"- Compare: https://github.com/{repo_slug}/compare/{previous_tag}...{tag}"
        )
    else:
        lines.append("- Previous tag: none")

    lines.append("")

    if not entries:
        lines.extend([
            "## Changes",
            "",
            "- No non-merge commits were found for this release window.",
        ])
        return "\n".join(lines) + "\n"

    for section in SECTION_ORDER:
        items = sections[section]
        if not items:
            continue
        lines.append(f"## {section}")
        lines.append("")
        lines.extend(items)
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    if len(sys.argv) not in (3, 4):
        print(
            "Usage: python3 scripts/generate_release_notes.py <tag> <notes-out> [changelog-out]",
            file=sys.stderr,
        )
        return 1

    tag = sys.argv[1].strip()
    notes_out = Path(sys.argv[2])
    changelog_out = Path(sys.argv[3]) if len(sys.argv) == 4 else None

    if not SEMVER_TAG_RE.fullmatch(tag):
        print(f"FAIL: tag must be a semver tag like v0.1.0 or v0.1.0-beta.1 (got {tag})", file=sys.stderr)
        return 1

    repo_slug = get_repo_slug()
    previous_tag = get_previous_tag(tag)
    range_spec = f"{previous_tag}..{tag}" if previous_tag else tag
    notes = render_release_notes(tag, repo_slug, previous_tag, get_commit_entries(range_spec))

    notes_out.parent.mkdir(parents=True, exist_ok=True)
    notes_out.write_text(notes, encoding="utf-8")

    if changelog_out is not None:
        changelog_out.parent.mkdir(parents=True, exist_ok=True)
        changelog_out.write_text(notes, encoding="utf-8")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
