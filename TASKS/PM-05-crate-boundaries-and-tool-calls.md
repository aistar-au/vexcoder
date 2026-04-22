# PM-05 -- Crate Boundaries and Structured Tool Calls

This task manifest records the repo-facing documentation alignment for PR #342
after the Tier 4 ratatui-stack overhaul.

The purpose is to keep vexapi grounded in conventional Rust CLI patterns
without copying wording or implementation material from any other project.
Comparable public codebases can inform dependency choices, but vexapi must
document its own rationale in neutral, repo-specific language.

## Scope

1. Replace vague or non-neutral wording in active repo docs.
2. Keep roadmap language explicit: current, merged, rejected, or next batch planned.
3. Document non-overlapping crate boundaries.
4. Document structured tool-call formats and parser boundaries.
5. Record active dependency decisions separately from implementation timing.

## Checklist

- [x] Replace `internal string surgery` with `internal text processing` in the
  active regex-lite documentation.
- [x] Replace active-roadmap wording such as `landed` with `merged` where the
  text describes completed PR state.
- [x] Replace roadmap sequencing language such as `deferred` / `follow-up` with
  `next batch planned` or `subsequent` in the active roadmap and ADR/task index
  docs when the text is describing scheduling rather than a locked design gate.
- [x] Keep formal ADR gate language unchanged where `deferred` is part of the
  decision semantics and changing it would change the meaning of the ADR.
- [x] Document the current structured tool-call formats:
  XML tags, JSON tool-call arrays, and content-block tool use.
- [x] Record current active crates that already define the runtime seam:
  `axum`, `rmcp`, `quick-xml`, `tower-http`, `fs2`, `portable-pty`, `similar`
  (`similar` replaces `diffy` as the diff algorithm in `src/edit_diff.rs`).
- [x] Record next-batch dependency candidates grounded in comparable Rust CLI
  patterns: `bm25`, `which`, `notify`.
- [x] Record overlap rejected now: `walkdir` duplicates `ignore`'s git-aware
  traversal and should not be added.
- [x] Document `rmcp` version pin (`1.2.x`) in the vexapi-specific crates table
  with a note that the version boundary matters for MCP wire protocol compatibility.

## Active Decisions Now

The following design choices are active in the repository now, even where the
crate addition itself is still tied to a later code batch:

- `axum` remains the HTTP server foundation for the local API surface.
- `rmcp` remains the MCP client transport layer.
- `quick-xml` remains the structured XML tag parser for fallback tool-call markup.
- `similar` is active as the diff algorithm in `src/edit_diff.rs`, replacing `diffy`.
  Generic line-diff algorithm with no branding dependency.
- `bm25` is accepted as the preferred next ranking layer for `codebase_search`
  once ADR-033 extends retrieval scoring.
- `which` is accepted as the preferred next executable-discovery helper for
  clearer `git` and tool resolution failures.
- `notify` is accepted as the preferred next filesystem-watch layer when
  `git_rollup` grows watch-mode or invalidation support.

## Rejected Now

- `walkdir` is rejected now because `ignore` already provides recursive,
  gitignore-aware traversal. Adding `walkdir` would create overlapping path
  traversal semantics with no compensating benefit.

## Why Accepted Does Not Mean Unused Dependency Now

For vexapi, accepted means the design choice is settled. It does not mean a
crate is added immediately and left unused.

Dependency additions stay coupled to a live runtime seam and test coverage so
the tree does not accumulate unused crates:

- `similar` lands when the transcript diff renderer is rewired to consume it.
- `bm25` lands when ADR-033 adds a ranking layer above structural retrieval.
- `which` lands when command-discovery failure paths are updated to use it.
- `notify` lands when watch-mode exists in `git_rollup` or adjacent invalidation paths.

## Notes

- This manifest tracks active repo-facing wording and boundary decisions.
- Formal ADR text keeps stronger terms such as `deferred` where those words are
  part of a locked decision rather than a wording choice.