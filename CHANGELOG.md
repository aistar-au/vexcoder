# Changelog

Release history is generated from semver tags.

For every tag that matches `v*`:

- `.github/workflows/release.yml` publishes the release artifacts.
- `scripts/generate_release_notes.py` renders the release notes body.
- The workflow uploads a matching `CHANGELOG-<tag>.md` asset for that tag.

To preview the generated changelog locally:

```bash
python3 scripts/generate_release_notes.py \
  v0.1.0-beta.4 \
  dist/release-notes.md \
  dist/CHANGELOG-v0.1.0-beta.4.md
```

The published release entry is the authoritative changelog for each cut.
