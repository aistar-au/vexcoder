# Agent Onboarding — aistar-au/vexcoder

**Repo:** `aistar-au/vexcoder`  
**Last updated:** 2026-03-05

---

## 1. Operating policy

Three rules apply before any task begins:

- Fetch and read all required skills in full before producing any output.
- Confirm with the user before any remote write (push, commit, PR create,
  PR update). A statement of intent is not confirmation.
- All file edits are exact unified diffs. No full-file rewrites from memory.
  No local filesystem writes for PR body work.

---

## 2. Skill architecture

Three skills govern this repo. They are separated by execution boundary:

| Skill | Path | Scope |
| :--- | :--- | :--- |
| `vex-local-bash` | `.agents/skills/vex-local-bash/SKILL.md` | Local drafting only. PR motivation bodies, review text, inline comments. No remote writes. |
| `vex-remote-contract` | `.agents/skills/vex-remote-contract/SKILL.md` | Batch dispatch, branch verification, raw URL checks, repo map gate, merge loop, and PR evidence collection. |
| `pull-request` | `.agents/skills/pr-motivation-body/SKILL.md` | Remote PR posting and review submission via GitHub MCP. Source verification before asserting facts. |

And one reference document:

| Reference | Path | Scope |
| :--- | :--- | :--- |
| PR remote guardrails | `.agents/skills/vex-remote-contract/references/pr-remote-guardrails.md` | Evidence and assertion rules for all remote PR writes. |

---

## 3. Required load order (every session)

Load and read all of the following in full before producing any output:

1. `.agents/skills/vex-local-bash/SKILL.md`
2. `.agents/skills/vex-remote-contract/SKILL.md`
3. `.agents/skills/pr-motivation-body/SKILL.md`
4. `.agents/skills/vex-remote-contract/references/pr-remote-guardrails.md`

Do not produce dispatch prompts, diffs, reviews, or PR bodies until all four
files have been read completely.

---

## 4. Execution boundaries

The local/remote split is the most critical architectural rule in this repo.
Violating it causes unchecked writes or fact-free assertions.

### Local boundary (vex-local-bash)

- Draft PR text, review findings, and inline comments in the assistant response.
- Do not call any GitHub write API.
- Do not write to `/tmp` or any local path for PR body work.
- Hand off final text to the remote posting step after explicit user approval.

### Remote boundary (vex-remote-contract + pr-motivation-body)

- All GitHub writes use GitHub MCP only.
- Source-verify struct field names and subprocess routing claims against the
  merge commit SHA before including them in any posted text.
- Do not assert CI status unless the GitHub status API returns a completed
  success conclusion on the head SHA.
- Present full draft to user; wait for explicit confirmation before any write.

### Push boundary (vex-remote-contract Hard Rule 22)

- Total changed payload >= 50 KB: use local CLI (`gh`/`git` cherry-pick + push).
- Total changed payload < 50 KB: use MCP `push_files` in a single batched call.
- `Cargo.lock` alone can exceed the threshold; sum blob sizes before deciding.

---

## 5. Domain context

**Language and toolchain:** Rust, async via Tokio, TUI via Ratatui.

**Architecture decisions:** All significant design decisions are recorded as
ADR files under `TASKS/`. The active roadmap is
[ADR-023 deterministic edit loop](https://github.com/aistar-au/vexcoder/blob/main/TASKS/ADR-023-deterministic-edit-loop.md).
Read the relevant ADR before writing any dispatch or PR body.

**Module boundaries enforced by CI:**

- `src/runtime/` — no `ratatui` or `crossterm` imports
- `src/app.rs` — TUI mode and slash-command dispatch
- `src/tools/` — tool operator and workspace-root confinement
- `src/state/` — persisted task and conversation state
- `TASKS/` — ADRs and dispatch task files
- `.agents/skills/` — agent skills (this directory)

**Canonical repo map:** `TASKS/completed/REPO-RAW-URL-MAP.md` — must be
checked for drift before every PR (Hard Rule 8).

---

## 6. Execution guardrails (machine-checkable)

Violation of any of these is a hard stop requiring user intervention.

- No emoji or Unicode status symbols in any output channel (Hard Rule 17).
- No CI status claims unless GitHub status API returns completed success on
  head SHA.
- No struct field names or subprocess routing claims without GitHub MCP
  source verification at the merge commit SHA.
- No nested markdown links (`[[text](url)](url)` is malformed).
- No numbered fix lists in review findings.
- `branch_summary.sh --write-pr-body` is disabled; the script dies on that
  flag. Use GitHub MCP for all PR body writes (Hard Rule 23).
- Merge commits only on `main` — no squash, no rebase (Hard Rule 2).
- Every push must be verified: local HEAD SHA must equal `origin/<branch>` SHA
  (Hard Rule 10).
- `vex-local-bash` never calls GitHub write APIs.
- `pr-motivation-body` never writes to local files or `/tmp`.

---

## 7. Workflow loop (abbreviated)

```
Step 0   SYNC        git fetch origin --prune; checkout target branch
Step 0.5 RECOVER     Resurrect deleted branch if needed (Priority 1 CLI first)
Step 1   READ        Fetch ADR and task files via GitHub MCP
Step 2   DISPATCH    Draft batch dispatch prompt (vex-local-bash)
Step 3   VERIFY      Second-agent review of dispatch
Step 4   EXECUTE     Code, test, push (local CLI)
Step 5   URL MAP     gen_verification_urls.sh
Step 6   RAW CHECK   verify_raw_urls.sh --compare
Step 7   DIFF CHECK  verify_diff_url.sh
Step 8   CI GATE     clippy + rustfmt + tests
Step 9   MAP GATE    update_repo_raw_url_map.sh --check
Step 10  PR BODY     Draft via vex-local-bash; post via pr-motivation-body MCP
Step 11  MERGE       merge --no-ff; verify SHAs; commit hygiene gate
Step 12  POST-MERGE  Re-fetch raw map URL; verify files at merge commit SHA
```

Full detail for each step is in `vex-remote-contract/SKILL.md`.

---

## 8. Session checklist

Required before the first batch action in any session:

- [ ] `vex-local-bash/SKILL.md` loaded and read in full
- [ ] `vex-remote-contract/SKILL.md` loaded and read in full
- [ ] `pr-motivation-body/SKILL.md` loaded and read in full
- [ ] `pr-remote-guardrails.md` loaded and read in full
- [ ] Current `main` HEAD SHA confirmed via GitHub MCP
- [ ] Relevant ADR fetched and read
- [ ] `REPO-RAW-URL-MAP.md` drift check run (`--check` flag)
- [ ] User confirmation received before any write action

---

## 9. Calibration tasks (run at session start)

These are read-only checks that verify policy compliance before any batch work.

1. Fetch `vex-remote-contract/SKILL.md` and confirm Hard Rule 23 is present.
2. Fetch `vex-local-bash/SKILL.md` and confirm the Local boundary section is
   present and that the skill explicitly prohibits GitHub write API calls.
3. Fetch `pr-remote-guardrails.md` and confirm the Evidence rules section is
   present.
4. Fetch `main` HEAD and report the current SHA and last commit message.
5. Fetch `TASKS/completed/REPO-RAW-URL-MAP.md` and confirm the file exists.

Report all five as confirmed before proceeding to any batch task.

---

## 10. Skill compliance failures — record and fix

After each batch, record any compliance failure as a finding in the PR review
using the Finding format from `vex-local-bash/SKILL.md`. Categories:

- Wrong field name asserted without source verification
- CI status claimed without GitHub API evidence
- Local file write for PR body content
- GitHub write API called from vex-local-bash
- Emoji or Unicode symbol in output
- Nested markdown link produced
- Skill file rewritten in full rather than patched

Skill changes follow the same exact-diff-via-MCP workflow as source changes.
No skill file is rewritten from memory.
