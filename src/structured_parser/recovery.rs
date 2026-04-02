//! Partial-token recovery strategies.
//!
//! When a streaming structured-output parser encounters malformed segments,
//! these strategies determine how to continue parsing rather than failing
//! outright.  This bridges the gap where vexcoder previously treated all
//! malformed segments as plain text with no attempt at recovery.

/// Describes the action the parser should take upon encountering a
/// structural error in the token stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Skip the offending byte(s) and continue parsing.
    Skip { bytes: usize },
    /// Insert synthetic content to repair the structure (e.g., a missing
    /// closing brace or tag).
    Insert { content: String },
    /// Replace the offending segment with a corrected version.
    Replace {
        original_len: usize,
        replacement: String,
    },
    /// Abandon the current structured block and emit the remainder as
    /// plain text.
    AbandonBlock,
    /// The error is unrecoverable; halt parsing.
    Fatal { message: String },
}

/// Strategy interface for recovery decision-making.
pub trait RecoveryStrategy: Send + Sync {
    /// Given the parser's current state and the malformed input, decide
    /// what recovery action to take.
    fn decide(
        &self,
        context: &RecoveryContext,
    ) -> RecoveryAction;
}

/// Context provided to a [`RecoveryStrategy`] when making a decision.
#[derive(Debug, Clone)]
pub struct RecoveryContext {
    /// The byte offset where the error occurred.
    pub offset: usize,
    /// The error description.
    pub message: String,
    /// How many recovery attempts have been made so far in this parse.
    pub attempt_count: usize,
    /// The last few characters before the error, for context.
    pub preceding_context: String,
    /// The offending characters.
    pub offending: String,
}

/// Default recovery strategy: tolerant of up to 3 errors, then abandons.
pub struct TokenRecovery {
    max_attempts: usize,
}

impl TokenRecovery {
    pub fn new(max_attempts: usize) -> Self {
        Self { max_attempts }
    }
}

impl Default for TokenRecovery {
    fn default() -> Self {
        Self::new(3)
    }
}

impl RecoveryStrategy for TokenRecovery {
    fn decide(&self, context: &RecoveryContext) -> RecoveryAction {
        if context.attempt_count >= self.max_attempts {
            return RecoveryAction::AbandonBlock;
        }

        // Heuristic: try to insert closing structures for common errors.
        let offending = &context.offending;

        // Unclosed JSON string.
        if context.message.contains("string") || offending.contains('"') {
            return RecoveryAction::Insert {
                content: "\"".to_string(),
            };
        }

        // Unclosed JSON object.
        if context.message.contains("object") || offending.contains('{') {
            return RecoveryAction::Insert {
                content: "}".to_string(),
            };
        }

        // Unclosed JSON array.
        if context.message.contains("array") || offending.contains('[') {
            return RecoveryAction::Insert {
                content: "]".to_string(),
            };
        }

        // Mismatched XML tag.
        if context.message.contains("tag") || offending.contains('<') {
            return RecoveryAction::Insert {
                content: ">".to_string(),
            };
        }

        // Default: skip the offending byte.
        RecoveryAction::Skip {
            bytes: offending.len().max(1),
        }
    }
}

/// Strict recovery strategy: never recovers, always fatal.
pub struct StrictRecovery;

impl RecoveryStrategy for StrictRecovery {
    fn decide(&self, context: &RecoveryContext) -> RecoveryAction {
        RecoveryAction::Fatal {
            message: context.message.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_recovery_skips_unknown_error() {
        let recovery = TokenRecovery::new(3);
        let ctx = RecoveryContext {
            offset: 42,
            message: "unknown error".into(),
            attempt_count: 0,
            preceding_context: "abcde".into(),
            offending: "x".into(),
        };
        let action = recovery.decide(&ctx);
        assert!(matches!(action, RecoveryAction::Skip { bytes: 1 }));
    }

    #[test]
    fn token_recovery_inserts_closing_brace() {
        let recovery = TokenRecovery::new(3);
        let ctx = RecoveryContext {
            offset: 10,
            message: "unexpected end inside object".into(),
            attempt_count: 0,
            preceding_context: "{\"key\":".into(),
            offending: "{".into(),
        };
        let action = recovery.decide(&ctx);
        assert!(matches!(action, RecoveryAction::Insert { content } if content == "}"));
    }

    #[test]
    fn token_recovery_abandons_after_max_attempts() {
        let recovery = TokenRecovery::new(2);
        let ctx = RecoveryContext {
            offset: 100,
            message: "repeated error".into(),
            attempt_count: 2,
            preceding_context: "...".into(),
            offending: "?".into(),
        };
        let action = recovery.decide(&ctx);
        assert!(matches!(action, RecoveryAction::AbandonBlock));
    }

    #[test]
    fn strict_recovery_is_always_fatal() {
        let recovery = StrictRecovery;
        let ctx = RecoveryContext {
            offset: 0,
            message: "any error".into(),
            attempt_count: 0,
            preceding_context: String::new(),
            offending: "x".into(),
        };
        let action = recovery.decide(&ctx);
        assert!(matches!(action, RecoveryAction::Fatal { .. }));
    }
}
