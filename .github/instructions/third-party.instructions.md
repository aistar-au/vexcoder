---
applyTo: "vendor/**,third_party/**,**/generated/**"
---

## Vendored, third-party, and generated content

### Editing policy

- Avoid editing these files unless the task explicitly requires it.
- If editing is required, keep changes minimal and document why the edit is
  necessary in both the code and the pull request.
- Preserve upstream structure, file layout, and naming conventions.

### License and attribution

- Preserve all existing license headers, copyright notices, and attribution
  comments. Never remove or alter them.
- Do not copy content from external sources into these paths without clear
  need, license compatibility, and explicit attribution.
- When the upstream license requires notice preservation (MIT, BSD, Apache-2.0),
  verify that the notice file is present and accurate after any modification.

### Maintenance and provenance

- Prefer fixing local integration code instead of modifying vendored sources.
  Patches to vendored code create maintenance burden on every upstream update.
- If a vendored file must be patched, keep the patch as a separate clearly
  marked diff or document the change in a `PATCHES.md` or equivalent so future
  updates can reapply it.
- If a change in these paths could create provenance or maintenance risk, call
  it out explicitly in the pull request under Risks.

### Generated files

- Do not hand-edit generated files. Modify the generator input or
  configuration instead and regenerate.
- If the generator is unavailable or the edit is urgent, document the manual
  change and file a follow-up to regenerate properly.
