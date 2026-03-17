# VexCoder Docs Contract

Public docs in `vexcoder` should answer three questions quickly:

1. How do I build it from source on my platform?
2. How do I initialize and configure it for this workspace?
3. How do I use the local API, auth, and command surface safely?

## Voice

- User-facing, not internal.
- Factual, not promotional.
- Less technical than ADR text, but still precise.

## Always verify

- command names
- flags
- config keys
- default paths
- local API behavior
- auth requirements
- TLS requirements

## Never introduce

- ADR references in public docs unless the page is explicitly about architecture
- internal dispatch terminology
- speculative future features
- unsupported examples
- stale path references
