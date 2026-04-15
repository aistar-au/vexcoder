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

Numeric pre-release identifiers must not contain leading zeros. Use `rc.1`,
not `rc.01`.

The canonical version lives in `Cargo.toml` under `[package] version`. All
tags, archive names, and release artifacts derive from this single source.

Dependency maintenance is separate from the release version bump. Direct crate
requirements live in the root `Cargo.toml` `[workspace.dependencies]` table;
use `make deps-deny`, `make deps-audit`, `make deps-plan`, and `make deps-upgrade`
(documented in `docs/src/dependency-upgrades.md`) for dependency work.
`make bump` changes the package version only.

### Pre-release progression

```
0.1.0-alpha.1 -> 0.1.0-alpha.2 -> ... -> 0.1.0-beta.1 -> 0.1.0-beta.2 -> ... -> 0.1.0-rc.1 -> 0.1.0
```

Pre-release versions use dot-separated numeric identifiers after the
pre-release label. This ensures correct semver precedence ordering:
`0.1.0-alpha.2 < 0.1.0-alpha.3 < 0.1.0-beta.1 < 0.1.0-beta.2 < 0.1.0-rc.1 < 0.1.0`.

---

## Git tag conventions

### Semver tags

- Tags use the `v` prefix: `v0.1.0-beta.1`, `v1.0.0`.
- Tags are **annotated**, not lightweight, and include a short summary.
- Tags are applied only to commits on `main` after the PR merge.
- Tags are never moved or deleted once pushed. If a tag-triggered release
  fails after the tag is published, land the fix and cut the next prerelease
  or patch tag instead of retagging an existing version.
- Tag names must match the `Cargo.toml` version exactly (with the `v` prefix):
  if `Cargo.toml` says `<current-version>`, the tag is `v<current-version>`.
- Create and push the tag locally from a synced `main` checkout. The tag is a
  post-merge release action, not a separate PR patch.

### Non-semver tags

- **Short-SHA tags** (7 hex characters, e.g. `bfb531d`) are created
  manually by the operator after a merge to `main`. They are immutable
  once pushed and trigger the release pipeline to produce a snapshot
  pre-release. See "Creating a short-SHA tag" below.
- **Nightly snapshot tags** are created automatically by the scheduled
  `.github/workflows/nightly.yml` workflow. Each nightly run creates a
  short-SHA tag from the current HEAD of `main` if one does not already
  exist. The tag push triggers the release pipeline to produce a snapshot
  pre-release. See "Automated nightly builds" below.
- Non-semver tags do not require a `Cargo.toml` version match. The release
  pipeline skips the version alignment check for these tags.

### Creating a short-SHA tag

```bash
git switch main
git pull --ff-only origin main

# Derive the 7-character short SHA of the merge commit
short_sha="$(git rev-parse HEAD)"; short_sha="${short_sha:0:7}"
echo "Short SHA: ${short_sha}"

# Create an annotated snapshot tag
git tag -a "${short_sha}" -m "Snapshot ${short_sha}"
git push origin "${short_sha}"
```

### Creating a semver tag

```bash
git switch main
git pull --ff-only origin main

# Verify the version in Cargo.toml matches the intended tag
grep '^version' Cargo.toml

# Create an annotated tag
git tag -a v<current-version> -m "Release v<current-version>"

# Push the tag
git push origin v<current-version>
```

---

## Release checklist

### Before merge

1. Run the automated version bump on the feature branch:
   ```bash
  make bump V=<new-version>
   ```
  This updates `Cargo.toml` and `Cargo.lock` in one step. See
  `scripts/bump-version.sh` for details.
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
  git ls-remote --tags origin | grep v<current-version>
    ```
11. Confirm `.github/workflows/release.yml` completed successfully for the tag.
12. Verify the workflow published the release entry, attached the platform
  archives, the macOS `.dmg` assets, and the matching
  `CHANGELOG-v<current-tag>.md` asset.

### After merge (optional snapshot release)

To produce an immediate snapshot release from the merge commit, create a
short-SHA tag manually (see "Creating a short-SHA tag" above).

A nightly snapshot tag is created automatically by the scheduled
`.github/workflows/nightly.yml` workflow. GitHub's server-side scheduler
evaluates the cron expression on the default branch and provisions a runner
when it fires. The workflow creates a short-SHA tag if one does not already
exist for the current HEAD, and the tag push triggers the release pipeline.
No manual action is needed for nightly builds.

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
2. Enter the new version (e.g. `0.1.0-rc.1`). No `v` prefix.
3. The workflow runs `scripts/bump-version.sh`, commits the version bump, and
  opens a PR targeting `main`.
4. Review and merge the PR.
5. After merge, create and push the annotated tag locally (see above). This is
   the release step; do not open another PR just for the tag.

This replaces the manual `make bump V=<version>` step for operators who
prefer a fully browser-based release flow.

---

## Automated nightly builds

`.github/workflows/nightly.yml` runs on a daily GitHub-hosted schedule on
the default branch. GitHub's server-side scheduler evaluates the cron
expression and provisions a runner when it fires — no external trigger or
always-on process is required.

Each run derives the 7-character short SHA of the default branch HEAD and
checks whether that tag already exists. If the tag is new, the workflow
creates an annotated short-SHA tag and pushes it. The tag push triggers
`.github/workflows/release.yml` to produce a snapshot pre-release. If the
tag already exists (i.e. no new commits since the last nightly), the run
skips gracefully.

The workflow uses the `NIGHTLY_CHANNEL_GIT_TAG_TOKEN` repository secret
(a PAT with `repo` scope) so that the tag push event triggers the
downstream release workflow. The default `GITHUB_TOKEN` would suppress
downstream workflow triggers.

Short-SHA snapshot tags can also be created manually by the operator via
`git tag` and `git push` (see "Creating a short-SHA tag" above).
Each pushed short-SHA tag triggers the release workflow automatically.

---

## Automated release workflow

`.github/workflows/release.yml` triggers on tag pushes matching semver
(`v*`), short-SHA (7 hex characters), and channel names (`nightly`).
It also supports `workflow_dispatch` for re-running a release from the
Actions UI without creating a new tag.
The workflow:

1. Builds release archives for 6 targets (Linux musl x86\_64 + aarch64,
   macOS x86\_64 + aarch64, Windows MSVC + GNU).
  The tag workflow packages both Windows variants from the already-validated
  commit and does not re-run the full Windows gate inside the packaging step.
  It also assembles per-architecture macOS `.dmg` bundles from the reviewed
  binaries. When Apple signing credentials are absent, the packaging lane still
  publishes clearly labelled unsigned development builds rather than skipping
  the macOS artifacts silently.
2. Signs archives with Sigstore cosign (keyless OIDC-backed bundles).
3. Generates release notes from the previous semver tag to the pushed tag.
4. Creates new release entries with the full asset set in a single publish
  step so immutable releases are populated on first publish, and verifies
  existing releases already contain the expected asset set before treating a
  re-run as complete.
5. Attaches all archives, macOS `.dmg` assets, checksums, signature bundles,
  and a generated `CHANGELOG-<tag>.md` asset.
6. Semver pre-release tags (containing `alpha`, `beta`, or `rc`) and short-SHA
  tags are automatically marked as pre-releases.

### Tag format summary

| Format | Example | Release type | Cargo.toml check |
| :--- | :--- | :--- | :--- |
| `v<semver>` | `v0.1.0-rc.8` | Stable or pre-release | Must match |
| 7-char hex SHA | `bfb531d` | Pre-release (snapshot) | Skipped |

Manual dispatch is available for re-running a failed release without
re-tagging when the tagged commit already contains the required fix and the
expected assets were published with the original release entry. If an immutable
release already exists without the required assets, land the fix and cut the
next prerelease or patch tag instead of moving the existing tag.

The packaging scripts in `scripts/` derive the archive name from the tag
for non-semver tags and from `Cargo.toml` for semver tags. Semver tags
reject mismatched tag inputs to prevent version drift between the binary
and the tag.

The package-manager tap formula template lives in `packaging/homebrew/vex.rb`.
After a tagged release publishes `checksums.txt`, run
`scripts/update_homebrew_formula.py <tag>` to materialize the formula for the
separate tap repository. Automatic repository-dispatch remains deferred until
that tap repository exists.

---

## References

- [Semantic Versioning 2.0.0](https://semver.org/)
- [Conventional Commits](https://www.conventionalcommits.org/) (recommended
  for commit messages)
- [Keep a Changelog](https://keepachangelog.com/) (useful guidance for the
  generated release notes format)
