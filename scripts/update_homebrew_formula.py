#!/usr/bin/env python3
"""Update the Homebrew formula template with release SHA256 checksums.

Usage:
    python3 scripts/update_homebrew_formula.py <version-tag> [--output <path>]

    version-tag   Git tag for the release (e.g. v0.1.0-beta.6).
    --output      Path to write the updated formula (default: stdout).

The script fetches SHA256 checksums from the checksums.txt asset published
on the GitHub release for <version-tag>, substitutes all __PLACEHOLDER__
values in packaging/homebrew/vex.rb, and writes the result.

Required checksums (targets that the formula references):
    aarch64-apple-darwin
    x86_64-apple-darwin
    aarch64-unknown-linux-musl
    x86_64-unknown-linux-musl

Exit codes:
    0  success
    1  usage error or I/O failure
    2  one or more expected checksums are missing from the release
"""

from __future__ import annotations

import argparse
import re
import sys
import urllib.request
from pathlib import Path

REPO = "aistar-au/vexcoder"
FORMULA_TEMPLATE = Path(__file__).parent.parent / "packaging" / "homebrew" / "vex.rb"

PLACEHOLDER_MAP = {
    "__SHA256_AARCH64_APPLE_DARWIN__": "aarch64-apple-darwin",
    "__SHA256_X86_64_APPLE_DARWIN__": "x86_64-apple-darwin",
    "__SHA256_AARCH64_UNKNOWN_LINUX_MUSL__": "aarch64-unknown-linux-musl",
    "__SHA256_X86_64_UNKNOWN_LINUX_MUSL__": "x86_64-unknown-linux-musl",
}


def fetch_checksums(version: str) -> dict[str, str]:
    """Fetch the checksums.txt from the GitHub release and parse it.

    Returns a mapping from target-suffix -> sha256 hex string.
    The checksums.txt lines are expected to have the format:
        <sha256>  vex-<version>-<target>.tar.gz
    """
    url = (
        f"https://github.com/{REPO}/releases/download/{version}/checksums.txt"
    )
    try:
        with urllib.request.urlopen(url, timeout=30) as resp:
            content = resp.read().decode()
    except Exception as exc:
        sys.exit(f"error: failed to fetch {url}: {exc}")

    checksums: dict[str, str] = {}
    for line in content.splitlines():
        line = line.strip()
        if not line:
            continue
        parts = line.split()
        if len(parts) != 2:
            continue
        sha, filename = parts
        # filename: vex-<version>-<target>.tar.gz
        m = re.match(r"vex-[^-].*?-(.+)\.tar\.gz$", filename)
        if m:
            checksums[m.group(1)] = sha
    return checksums


def substitute(template: str, version: str, checksums: dict[str, str]) -> str:
    result = template.replace("__VERSION__", version)
    missing: list[str] = []
    for placeholder, target in PLACEHOLDER_MAP.items():
        sha = checksums.get(target)
        if sha is None:
            missing.append(target)
        else:
            result = result.replace(placeholder, sha)
    if missing:
        sys.exit(
            f"error: checksums missing for targets: {', '.join(missing)}\n"
            "       ensure the release has a checksums.txt covering all formula targets"
        )
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="release tag, e.g. v0.1.0-beta.6")
    parser.add_argument("--output", default=None, help="output path (default: stdout)")
    args = parser.parse_args()

    version = args.version
    if not version.startswith("v"):
        sys.exit("error: version must start with 'v', e.g. v0.1.0-beta.6")

    template = FORMULA_TEMPLATE.read_text()
    checksums = fetch_checksums(version)
    updated = substitute(template, version, checksums)

    if args.output:
        out = Path(args.output)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(updated)
        print(f"written: {out}", file=sys.stderr)
    else:
        sys.stdout.write(updated)


if __name__ == "__main__":
    main()
