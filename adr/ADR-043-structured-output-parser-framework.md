# ADR-043: Structured Output Parser Framework

- **Status:** Active
- **Date:** 2026-04-02
- **Deciders:** Core maintainer
- **Depends on:** ADR-029
- **Supersedes:** None
- **Superseded by:** None

## Context

A gap analysis against an established open-source inference engine's
structured-output parsing framework identified seven capabilities absent
from vexcoder's parser:

1. Grammar-based constrained parsing (BNF/regex)
2. Incremental JSON streaming validation with recovery
3. XML/tag tree parsing with nested structure validation
4. Partial-token recovery after malformed segments
5. Multi-format parsing modes (JSON, XML, Grammar, Regex, Tag)
6. Structured output guarantees (strict/best-effort enforcement)
7. Fine-grained parser callbacks (token-level events)

vexcoder's existing parsing is a transport-level parser spread across
`api/stream.rs` (SSE frame parsing), `tool_call_parser.rs` (Tagged/XML
tool-call extraction), and `stream_block.rs` (block lifecycle).  This is
sufficient for a CLI assistant consuming well-formed model output, but
not for robust structured output from local models that may emit
malformed JSON, unclosed tags, or interleaved thinking/tool blocks.

## Decision

Introduce a `structured_parser` module (`src/structured_parser/`) that
provides a unified, mode-aware parser framework.

### Sub-modules

| Module | Purpose | Gap addressed |
|:---|:---|:---|
| `grammar.rs` | BNF-like grammar rules with rule-stack engine | GAP 1 |
| `json_validator.rs` | Streaming JSON validator with depth tracking, escape handling, best-effort recovery | GAP 2 |
| `tag_tree.rs` | Nested XML/tag tree builder with `TagStack` nesting validator | GAP 3 |
| `recovery.rs` | Recovery strategy trait with `TokenRecovery` (tolerant) and `StrictRecovery` policies | GAP 4 |
| `modes.rs` | `ParseMode` enum (Json/Xml/Grammar/Regex/Tag/Passthrough) and `StructuredParser` dispatcher | GAP 5 |
| `validate.rs` | `OutputGuarantee` levels (None/BestEffort/Strict) and `ValidationResult` | GAP 6 |
| `callbacks.rs` | `ParserEvent` enum and `ParserCallback` trait for fine-grained events | GAP 7 |

### Design principles

- **No new crate dependencies.**  The grammar engine uses a conservative
  prefix matcher instead of the `regex` crate; downstream validators
  (`serde_json`, `quick-xml`) remain the source of truth for format
  correctness.
- **Composable with existing parsers.**  `StructuredParser` can wrap the
  existing `StreamParser` and `ToolCallParser` chain; it does not replace
  them.
- **Incremental by design.**  Every sub-parser accepts token-at-a-time
  input via a `feed()` method and maintains state across calls.
- **Recovery-first.**  The `RecoveryStrategy` trait lets callers choose
  between tolerant (skip/insert), strict (fatal), or custom policies.

### Environment controls

The module defines a `ParseMode` API and an internal `VEX_PARSE_MODE` lookup
helper for future integration work. PR #314 does not yet route the primary
`send_message` runtime through this path or expose it in the main configuration
surface.

### What this ADR does *not* cover

- **Grammar-constrained decoding** (guiding the sampler).  The grammar
  engine is a post-hoc validator; wiring it into a sampling loop requires
  inference-engine integration outside this scope.
- **Full NFA-based prefix matching.**  The grammar engine's prefix check
  is intentionally conservative; a proper NFA matcher is future work.

## Consequences

- All seven parity gaps now have structural scaffolding in the codebase.
- Local-model integrations still use the existing tagged or hybrid tool parser
  path until a later change wires `StructuredParser` into the runtime.
- The `ParserCallback` trait enables future TUI features like real-time
  structured-output progress indicators.
- Test coverage: 40 new unit tests across all sub-modules.
