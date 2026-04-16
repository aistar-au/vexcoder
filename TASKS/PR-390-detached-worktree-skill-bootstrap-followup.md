# PR 390 Replacement Follow-Up -- Detached Worktree Skill Bootstrap

Branch: `work/vexcoder-eventsource-pr390-replacement`

This note records a follow-up that is adjacent to the replacement PR but not
part of the EventSource or runtime-envelope code diff.

## Context

- Local operator sessions may run from sandbox worktrees under `.sandboxes/`.
- `AGENTS.md` local bootstrap instructions still use relative
  `../vexdraft/.agents/skills/...` paths.
- Repository-hosted sessions must continue to stop before any private-skill
  bootstrap.
- This replacement PR does not change bootstrap behavior; it only records the
  detached-worktree audit as explicit follow-up work.

## Follow-Up

1. Confirm the local bootstrap paths resolve correctly from detached
   worktrees, regular worktrees, and the primary checkout.
2. If bootstrap depends on current working-directory shape, anchor it to the
   repository root or an explicit configuration path instead of `../...`
   assumptions.
3. Keep the hosted-session short circuit intact so repository-hosted runs still
   ignore local bootstrap instructions.
4. Add text-only validation notes for the audited bootstrap paths and any
   confirmed failure mode.

## Non-goals

- No private-skill bootstrap changes in this PR.
- No hosted-session behavior changes in this PR.
- No expansion of the EventSource or runtime-streaming code diff.