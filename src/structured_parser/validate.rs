//! Structured output guarantees and validation results.
//!
//! Defines the enforcement layer that sits between the parser and the
//! consumer, ensuring that model output conforms to the declared format.

/// Output guarantee level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputGuarantee {
    /// No enforcement — accept whatever the model produces.
    None,
    /// Best-effort: attempt to parse and recover, but don't reject output.
    BestEffort,
    /// Strict: reject output that doesn't conform to the declared format.
    Strict,
}

/// The outcome of a validation check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationOutcome {
    /// The output is structurally valid (or complete).
    Valid,
    /// The output is a valid prefix; more tokens are expected.
    Partial,
    /// An error was detected but recovery was applied.
    Recovered,
    /// The output is structurally invalid.
    Invalid,
}

/// Result of validating a token or the final output.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub outcome: ValidationOutcome,
    pub message: Option<String>,
}

impl ValidationResult {
    /// Shorthand for a valid result with no message.
    pub fn valid() -> Self {
        Self {
            outcome: ValidationOutcome::Valid,
            message: None,
        }
    }

    /// Shorthand for an error result with a message.
    pub fn error(message: String) -> Self {
        Self {
            outcome: ValidationOutcome::Invalid,
            message: Some(message),
        }
    }

    /// Returns `true` if the output is acceptable (valid, partial, or recovered).
    pub fn is_acceptable(&self) -> bool {
        matches!(
            self.outcome,
            ValidationOutcome::Valid | ValidationOutcome::Partial | ValidationOutcome::Recovered
        )
    }

    /// Returns `true` if the validation detected an error.
    pub fn is_error(&self) -> bool {
        matches!(self.outcome, ValidationOutcome::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_result_is_acceptable() {
        assert!(ValidationResult::valid().is_acceptable());
        assert!(!ValidationResult::valid().is_error());
    }

    #[test]
    fn error_result_is_not_acceptable() {
        let r = ValidationResult::error("bad".into());
        assert!(!r.is_acceptable());
        assert!(r.is_error());
    }

    #[test]
    fn partial_is_acceptable() {
        let r = ValidationResult {
            outcome: ValidationOutcome::Partial,
            message: None,
        };
        assert!(r.is_acceptable());
    }

    #[test]
    fn recovered_is_acceptable() {
        let r = ValidationResult {
            outcome: ValidationOutcome::Recovered,
            message: Some("fixed".into()),
        };
        assert!(r.is_acceptable());
    }
}
