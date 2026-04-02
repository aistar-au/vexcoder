//! Structured output parser framework for vexcoder.
//!
//! Closes the parser gap between vexcoder's transport-level parsing and
//! a full structured-output framework.  This module provides:
//!
//! - **Grammar engine** – BNF-like and regex-based constrained parsing
//! - **JSON streaming validator** – incremental JSON validation with recovery
//! - **XML/tag tree parser** – nested tag support with stack-based validation
//! - **Partial-token recovery** – continuation after malformed segments
//! - **Multi-format modes** – JSON, XML, Grammar, Regex, Tag modes
//! - **Structured output guarantees** – enforcement layer over model output
//! - **Fine-grained parser callbacks** – event-driven structured output events
//!
//! ADR-043 documents the design rationale.

mod callbacks;
mod grammar;
mod json_validator;
mod modes;
mod recovery;
mod tag_tree;
mod validate;

pub use self::callbacks::{ParserCallback, ParserEvent};
pub use self::grammar::{Grammar, GrammarEngine, GrammarRule};
pub use self::json_validator::{JsonStreamValidator, JsonValidationState};
pub use self::modes::{ParseMode, StructuredParser};
pub use self::recovery::{RecoveryAction, RecoveryStrategy, TokenRecovery};
pub use self::tag_tree::{TagNode, TagStack, TagTreeParser};
pub use self::validate::{OutputGuarantee, ValidationOutcome, ValidationResult};

/// Re-export for ergonomic imports.
pub mod prelude {
    pub use super::callbacks::{ParserCallback, ParserEvent};
    pub use super::modes::{ParseMode, StructuredParser};
    pub use super::validate::ValidationResult;
}
