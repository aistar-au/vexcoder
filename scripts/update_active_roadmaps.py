#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import date
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
TASKS_DIR = ROOT / "TASKS"

CANONICAL_ADR_FILES = {
    "ADR-022": "ADR-022-free-open-coding-agent-roadmap.md",
    "ADR-023": "ADR-023-deterministic-edit-loop.md",
    "ADR-024": "ADR-024-zero-licensing-cost-agent-parity-gaps.md",
}
AMENDMENT_FILES = {"ADR-022": "ADR-022-amendment-2026-03-03.md"}

NOTES = {
    "ADR-022": "2026-03-03 amendment constrains first-milestone scope and later-surface sequencing.",
    "ADR-023": "Deterministic edit loop implementation track.",
    "ADR-024": "Parity-gap roadmap; source of gap sequencing and constraints.",
}


@dataclass(frozen=True)
class AdrInfo:
    adr_id: str
    path: Path
    title: str
    status: str
    note: str


def _canonical_adr_path(adr_id: str) -> Path:
    path = TASKS_DIR / CANONICAL_ADR_FILES[adr_id]
    if not path.exists():
        raise FileNotFoundError(f"Missing ADR file for {adr_id}: {path}")
    return path


def _amendment_path(adr_id: str) -> Path | None:
    name = AMENDMENT_FILES.get(adr_id)
    if name is None:
        return None
    path = TASKS_DIR / name
    return path if path.exists() else None


def _parse_status(text: str) -> str | None:
    for line in text.splitlines():
        stripped = line.strip()
        for prefix in (
            "**Status:**",
            "Status:",
            "**Amendment status:**",
            "Amendment status:",
        ):
            if stripped.startswith(prefix):
                return stripped[len(prefix) :].strip()
    return None


def _parse_adr(adr_id: str) -> AdrInfo:
    path = _canonical_adr_path(adr_id)
    text = path.read_text(encoding="utf-8")
    title = path.stem
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("# "):
            title = stripped[2:].strip()
            break
    status = _parse_status(text) or "Unknown"
    amendment = _amendment_path(adr_id)
    if amendment is not None:
        amendment_status = _parse_status(amendment.read_text(encoding="utf-8"))
        if amendment_status:
            status = f"{amendment_status} (amendment)"
    return AdrInfo(
        adr_id=adr_id,
        path=path,
        title=title,
        status=status,
        note=NOTES.get(adr_id, ""),
    )


def _replace_block(path: Path, marker: str, body: str) -> None:
    begin = f"<!-- AUTO:{marker}:BEGIN -->"
    end = f"<!-- AUTO:{marker}:END -->"
    text = path.read_text(encoding="utf-8")
    start = text.find(begin)
    finish = text.find(end)
    if start == -1 or finish == -1 or finish < start:
        raise RuntimeError(f"Missing AUTO block markers {marker} in {path}")
    finish += len(end)
    block = f"{begin}\n{body.rstrip()}\n{end}"
    updated = text[:start] + block + text[finish:]
    path.write_text(updated, encoding="utf-8")


def _replace_line_prefix(path: Path, prefix: str, new_line: str) -> None:
    lines = path.read_text(encoding="utf-8").splitlines()
    for idx, line in enumerate(lines):
        if line.startswith(prefix):
            lines[idx] = new_line
            path.write_text("\n".join(lines) + "\n", encoding="utf-8")
            return
    raise RuntimeError(f"Missing line prefix '{prefix}' in {path}")


def _render_active_table(rows: list[AdrInfo]) -> str:
    out = [
        "| ADR | Status | Notes |",
        "| :--- | :--- | :--- |",
    ]
    for row in rows:
        rel = row.path.relative_to(ROOT).as_posix()
        out.append(
            f"| [{row.adr_id}](https://github.com/aistar-au/vexcoder/blob/main/{rel}) | "
            f"{row.status} | {row.note} |"
        )
    return "\n".join(out)


def _render_summary(rows: list[AdrInfo]) -> str:
    return "\n".join([f"- `{row.adr_id}` - {row.status}" for row in rows])


def main() -> int:
    rows = [_parse_adr(adr_id) for adr_id in CANONICAL_ADR_FILES]

    active_file = ROOT / "TASKS/ACTIVE-ROADMAP.md"
    onboarding_file = ROOT / ".agents/onboarding.md"
    dispatch_file = ROOT / "TASKS/TASKS-DISPATCH-MAP.md"

    _replace_line_prefix(
        active_file,
        "**Last updated:**",
        f"**Last updated:** {date.today().isoformat()}",
    )
    _replace_block(active_file, "ACTIVE_ADRS", _render_active_table(rows))
    _replace_block(onboarding_file, "ACTIVE_ROADMAPS", _render_summary(rows))
    _replace_block(dispatch_file, "ACTIVE_ROADMAPS", _render_summary(rows))

    print("Updated active roadmap blocks:")
    print("- TASKS/ACTIVE-ROADMAP.md")
    print("- .agents/onboarding.md")
    print("- TASKS/TASKS-DISPATCH-MAP.md")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"roadmap-sync error: {exc}", file=sys.stderr)
        raise SystemExit(1)
