# PR 347 Follow-Up

## Reference notes

- [x] Review a clean-room reference CLI pattern for context handling and tool loops.
- [x] Confirm the session-log symptom: repeated read-only tool calls are more likely when large or low-context tool results are echoed back raw.
- [x] Confirm the ratatui surface should keep status, transcript, and composer on one canonical render path.

## Checklist

- [x] Resolve PR 347 branch, reviewer threads, and check state without merging.
- [x] Restore the ratatui task-surface status row on the primary render path.
- [x] Tighten tool-result history enrichment so the next model round gets request context plus bounded output previews.
- [x] Remove remaining banned naming hits and stale delta-path wording from tracked docs and ADRs.
- [ ] Rewrite the PR body to match the actual branch contents and follow repository structure.
- [ ] Run local validation, push the branch, watch checks to success, and close out any remaining automated review noise.
