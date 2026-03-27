# Releasing

This document defines the versioning, tagging, and release process for
`aistar-au/vexcoder`.

---

## Versioning scheme

This project follows [Semantic Versioning 2.0.0](https://semver.org/).

Format: `MAJOR.MINOR.PATCH[-PRERELEASE]`

- **MAJOR** — incompatible API or CLI changes.
- **MINOR** — backwards-compatible new functionality.
- **PATCH** — backwards-compatible bug fixes.
- **PRERELEASE** — optional dot-separated identifiers for pre-release builds
  (e.g. `alpha.3`, `beta.4`, `rc.1`).

The canonical version lives in `Cargo.toml` under `[package] version`. All
tags, archive names, and release artifacts derive from this single source.

### Pre-release progression

```
0.1.0-alpha.1 -> 0.1.0-alpha.2 -> ... -> 0.1.0-beta.1 -> 0.1.0-beta.2 -> 0.1.0-beta.3 -> 0.1.0-beta.4 -> 0.1.0-beta.6 -> 0.1.0-rc.1 -> 0.1.0
```

Pre-release versions use dot-separated numeric identifiers after the
pre-release label. This ensures correct semver precedence ordering:
`0.1.0-alpha.2 < 0.1.0-alpha.3 < 0.1.0-beta.1 < 0.1.0-beta.2 < 0.1.0-beta.3 < 0.1.0-beta.4 < 0.1.0-beta.6 < 0.1.0-rc.1 < 0.1.0`.

---

## Git tag conventions

- Tags use the `v` prefix: `v0.1.0-beta.1`, `v1.0.0`.
- Tags are **annotated**, not lightweight, and include a short summary.
- Tags are applied only to commits on `main` after the PR merge.
- Tags are never moved or deleted once pushed. If a tag-triggered release
  fails after the tag is published, land the fix and cut the next prerelease
  or patch tag instead of retagging an existing version.
- Tag names must match the `Cargo.toml` version exactly (with the `v` prefix):
  if `Cargo.toml` says `0.1.0-beta.6`, the tag is `v0.1.0-beta.6`.

### Creating a tag

```bash
git switch main
git pull --ff-only origin main

# Verify the version in Cargo.toml matches the intended tag
grep '^version' Cargo.toml

# Create an annotated tag
git tag -a v0.1.0-beta.6 -m "Release v0.1.0-beta.6"

# Push the tag
git push origin v0.1.0-beta.6
```

---

## Release checklist

### Before merge

1. Run the automated version bump on the feature branch:
   ```bash
  make bump V=<new-version>
   ```
   This updates `Cargo.toml`, `Cargo.lock`, `CONTRIBUTING.md`, and
   `RELEASING.md` in one step. See `scripts/bump-version.sh` for details.
2. Review the changes: `git diff`.
3. Verify all CI checks pass on the PR.
4. Run the local gate:
   ```bash
   make gate-fast
   ```
5. Run commit-debug if `src/` or `tests/` changed (see `CONTRIBUTING.md`).

### Merge and tag

6. Merge the PR into `main` with a merge commit.
7. Pull the merge commit locally:
   ```bash
   git switch main
   git pull --ff-only origin main
   ```
8. Verify the `Cargo.toml` version on the merge commit.
9. Create and push the annotated tag (see above).

### After tag push

10. Verify the tag exists on the remote:
    ```bash
  git ls-remote --tags origin | grep v0.1.0-beta.6
    ```
11. Confirm `.github/workflows/release.yml` completed successfully for the tag.
12. Verify the workflow published the release entry, attached the platform
  archives, and uploaded the matching `CHANGELOG-v0.1.0-beta.6.md` asset.

---

## Version bump rules

| Change type                        | Bump          | Example                      |
| :--------------------------------- | :------------ | :--------------------------- |
| Breaking CLI or API change         | MAJOR         | `1.0.0` -> `2.0.0`          |
| New feature, backwards-compatible  | MINOR         | `0.1.0` -> `0.2.0`          |
| Bug fix, no new features           | PATCH         | `0.1.1` -> `0.1.2`          |
| Pre-release iteration              | PRERELEASE    | `0.1.0-alpha.2` -> `alpha.3`|
| Stability promotion                | Drop/change   | `0.1.0-rc.1` -> `0.1.0`    |

During the `0.x` series, minor version bumps may include breaking changes as
the API stabilises. The pre-release suffix tracks iteration within a minor
version.

---

## Branch and PR conventions for releases

- Version bumps land as part of normal feature PRs, not as standalone
  "release PRs" (unless a release requires coordinated changes across
  multiple PRs).
- The merge commit on `main` is the tagged commit. Do not tag feature
  branches or intermediate commits.
- If a release is cut from a stabilisation branch (e.g. `release/0.2`),
  the same tag rules apply: annotated tag on the branch head after merge.

---

## Hotfix process

1. Branch from the tagged release commit:
   ```bash
  git checkout -b hotfix/v0.1.1 v0.1.0
   ```
2. Apply the minimal fix.
3. Bump the PATCH version in `Cargo.toml`.
4. Open a PR targeting `main` (or the release branch if applicable).
5. After merge, tag and release as above.

---

## Automated version bump

The `version-bump` workflow (`.github/workflows/version-bump.yml`) is a
manual dispatch workflow that automates the version bump process:

1. Go to **Actions > version-bump > Run workflow**.
2. Enter the new version (e.g. `0.1.0-beta.6`). No `v` prefix.
3. The workflow runs `scripts/bump-version.sh`, commits the changes, and
   opens a PR targeting `main`.
4. Review and merge the PR.
5. After merge, create and push the annotated tag (see above).

This replaces the manual `make bump V=<version>` step for operators who
prefer a fully browser-based release flow.

---

## Automated release workflow

`.github/workflows/release.yml` triggers on tag pushes matching `v*`.
The workflow:

1. Builds release archives for 5 targets (Linux musl x86\_64 + aarch64,
   macOS x86\_64 + aarch64, Windows MSVC).
  The tag workflow packages Windows from the already-validated commit and
  does not re-run the full Windows gate inside the packaging step.
2. Signs archives with Sigstore cosign (keyless OIDC-backed bundles).
3. Generates release notes from the previous semver tag to the pushed tag.
4. Creates new release entries with the full asset set in a single publish
  step so immutable releases are populated on first publish, and verifies
  existing releases already contain the expected asset set before treating a
  re-run as complete.
5. Attaches all archives, checksums, signature bundles, and a generated
  `CHANGELOG-<tag>.md` asset.
6. Pre-release tags (containing `alpha`, `beta`, or `rc`) are
   automatically marked as pre-releases.

Manual dispatch is available for re-running a failed release without
re-tagging when the tagged commit already contains the required fix and the
expected assets were published with the original release entry. If an immutable
release already exists without the required assets, land the fix and cut the
next prerelease or patch tag instead of moving the existing tag.

The packaging scripts in `scripts/` derive the archive name from
`Cargo.toml` and reject mismatched tag inputs to prevent version drift
between the binary and the tag.

---

## References

- [Semantic Versioning 2.0.0](https://semver.org/)
- [Conventional Commits](https://www.conventionalcommits.org/) (recommended
  for commit messages)
- [Keep a Changelog](https://keepachangelog.com/) (useful guidance for the
  generated release notes format)
