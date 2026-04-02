//! Multi-format parsing modes and the unified [`StructuredParser`].
//!
//! Provides a single entry-point parser that can operate in JSON, XML,
//! Grammar, Regex, or Tag mode, dispatching to the appropriate sub-parser
//! and enforcing the relevant structural guarantees.

use super::callbacks::{ParserCallback, ParserEvent};
use super::grammar::GrammarEngine;
use super::json_validator::{JsonStreamValidator, JsonValidationState};
use super::recovery::{RecoveryContext, RecoveryStrategy, TokenRecovery};
use super::tag_tree::TagTreeParser;
use super::validate::{OutputGuarantee, ValidationOutcome, ValidationResult};

/// The active parsing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    /// Validate and enforce strict JSON output.
    Json,
    /// Build and validate an XML/tag tree.
    Xml,
    /// Enforce output against a grammar definition.
    Grammar,
    /// Extract content matching a regex pattern.
    Regex,
    /// Tag-based segmentation (enhanced version of existing behaviour).
    Tag,
    /// Passthrough — no structured validation (existing behaviour).
    Passthrough,
}

impl ParseMode {
    /// Resolve mode from an environment variable or profile hint.
    pub fn from_env_or(profile: Option<&str>, default: Self) -> Self {
        match std::env::var("VEX_PARSE_MODE").ok().as_deref() {
            Some("json") => Self::Json,
            Some("xml") => Self::Xml,
            Some("grammar") => Self::Grammar,
            Some("regex") => Self::Regex,
            Some("tag") => Self::Tag,
            Some("passthrough") => Self::Passthrough,
            _ => match profile {
                Some("json") => Self::Json,
                Some("xml") => Self::Xml,
                Some("grammar") => Self::Grammar,
                Some("regex") => Self::Regex,
                Some("tag") => Self::Tag,
                _ => default,
            },
        }
    }
}

/// Unified structured parser that delegates to mode-specific sub-parsers.
pub struct StructuredParser {
    mode: ParseMode,
    json: Option<JsonStreamValidator>,
    xml: Option<TagTreeParser>,
    grammar: Option<GrammarEngine>,
    regex_pattern: Option<String>,
    tag: Option<TagTreeParser>,
    callbacks: Vec<Box<dyn ParserCallback>>,
    recovery: Box<dyn RecoveryStrategy>,
    guarantee: OutputGuarantee,
    /// Accumulated raw output for post-validation.
    raw: String,
    /// Error count for recovery tracking.
    error_count: usize,
}

impl StructuredParser {
    /// Create a new parser in the given mode with default settings.
    pub fn new(mode: ParseMode) -> Self {
        let mut parser = Self {
            mode,
            json: None,
            xml: None,
            grammar: None,
            regex_pattern: None,
            tag: None,
            callbacks: Vec::new(),
            recovery: Box::new(TokenRecovery::default()),
            guarantee: OutputGuarantee::BestEffort,
            raw: String::new(),
            error_count: 0,
        };

        match mode {
            ParseMode::Json => {
                parser.json = Some(JsonStreamValidator::new(true));
            }
            ParseMode::Xml | ParseMode::Tag => {
                let tree_parser = TagTreeParser::new();
                if mode == ParseMode::Xml {
                    parser.xml = Some(tree_parser);
                } else {
                    parser.tag = Some(tree_parser);
                }
            }
            ParseMode::Grammar | ParseMode::Regex | ParseMode::Passthrough => {}
        }

        parser
    }

    /// Set the output guarantee level.
    pub fn with_guarantee(mut self, guarantee: OutputGuarantee) -> Self {
        self.guarantee = guarantee;
        self
    }

    /// Set a custom recovery strategy.
    pub fn with_recovery(mut self, recovery: impl RecoveryStrategy + 'static) -> Self {
        self.recovery = Box::new(recovery);
        self
    }

    /// Set a grammar engine (for Grammar mode).
    pub fn with_grammar(mut self, engine: GrammarEngine) -> Self {
        self.grammar = Some(engine);
        self
    }

    /// Set a pattern string (for Regex mode).
    /// Uses simple substring matching; upgrade to an NFA when needed.
    pub fn with_regex(mut self, pattern: &str) -> Self {
        self.regex_pattern = Some(pattern.to_string());
        self
    }

    /// Register a parser callback for fine-grained events.
    pub fn add_callback(&mut self, cb: impl ParserCallback + 'static) {
        self.callbacks.push(Box::new(cb));
    }

    /// Feed a token into the parser.
    ///
    /// Returns a validation result indicating whether the accumulated
    /// output is structurally valid so far.
    pub fn feed(&mut self, token: &str) -> ValidationResult {
        self.raw.push_str(token);

        match self.mode {
            ParseMode::Json => self.feed_json(token),
            ParseMode::Xml => self.feed_xml(token),
            ParseMode::Grammar => self.feed_grammar(token),
            ParseMode::Regex => self.feed_regex(token),
            ParseMode::Tag => self.feed_tag(token),
            ParseMode::Passthrough => {
                self.emit(ParserEvent::TextDelta {
                    content: token.to_string(),
                });
                ValidationResult::valid()
            }
        }
    }

    fn feed_json(&mut self, token: &str) -> ValidationResult {
        let validator = self.json.as_mut().expect("JSON validator not initialised");
        let state = validator.feed(token);

        // Collect event and result before calling self.emit() to avoid
        // holding a mutable borrow on self.json across the call.
        let (event, result) = match &state {
            JsonValidationState::Complete => {
                let value = validator.best_effort_value();
                (
                    Some(ParserEvent::JsonComplete { value }),
                    ValidationResult::valid(),
                )
            }
            JsonValidationState::Partial(ctx) => (
                Some(ParserEvent::TextDelta {
                    content: token.to_string(),
                }),
                ValidationResult {
                    outcome: ValidationOutcome::Partial,
                    message: Some(format!("in {ctx}")),
                },
            ),
            JsonValidationState::Error { offset, message } => {
                self.error_count += 1;
                let recovery_ctx = RecoveryContext {
                    offset: *offset,
                    message: message.clone(),
                    attempt_count: self.error_count,
                    preceding_context: self.preceding_context(),
                    offending: token.to_string(),
                };
                let action = self.recovery.decide(&recovery_ctx);
                (
                    Some(ParserEvent::RecoveryAttempt {
                        action: format!("{action:?}"),
                    }),
                    ValidationResult::error(message.clone()),
                )
            }
            JsonValidationState::Recovered { offset: _, message } => {
                self.error_count += 1;
                (
                    Some(ParserEvent::RecoveryAttempt {
                        action: format!("recovered: {message}"),
                    }),
                    ValidationResult {
                        outcome: ValidationOutcome::Recovered,
                        message: Some(message.clone()),
                    },
                )
            }
            JsonValidationState::Empty => (None, ValidationResult::valid()),
        };

        if let Some(ev) = event {
            self.emit(ev);
        }
        result
    }

    fn feed_xml(&mut self, token: &str) -> ValidationResult {
        let parser = self.xml.as_mut().expect("XML parser not initialised");
        let nodes = parser.feed(token);

        // Collect events and build result while the mutable borrow is active,
        // then emit events after releasing it.
        let mut events: Vec<ParserEvent> = Vec::new();

        for node in &nodes {
            events.push(ParserEvent::TagClose {
                name: node.name.clone(),
            });
        }

        let result = if !parser.tag_stack().errors().is_empty() {
            let errors: Vec<String> = parser
                .tag_stack()
                .errors()
                .iter()
                .map(|e| e.to_string())
                .collect();
            ValidationResult::error(errors.join("; "))
        } else if nodes.is_empty() {
            if let Some(tag) = parser.tag_stack().current_tag() {
                events.push(ParserEvent::TagOpen {
                    name: tag.to_string(),
                });
            }
            ValidationResult {
                outcome: ValidationOutcome::Partial,
                message: None,
            }
        } else {
            ValidationResult::valid()
        };

        // Mutable borrow of self.xml is now dropped; safe to call self.emit.
        for ev in events {
            self.emit(ev);
        }
        result
    }

    fn feed_grammar(&mut self, token: &str) -> ValidationResult {
        let (matched, event) = if let Some(engine) = &mut self.grammar {
            if engine.feed(token) {
                (true, Some(ParserEvent::GrammarMatch { rule: String::new() }))
            } else {
                (false, None)
            }
        } else {
            return ValidationResult::error("no grammar engine configured".into());
        };

        if let Some(ev) = event {
            self.emit(ev);
        }
        if matched {
            ValidationResult::valid()
        } else {
            ValidationResult::error("token does not match grammar".into())
        }
    }

    fn feed_regex(&mut self, _token: &str) -> ValidationResult {
        if let Some(pattern) = &self.regex_pattern {
            // Check if the accumulated raw contains the pattern.
            if self.raw.contains(pattern.as_str()) {
                ValidationResult::valid()
            } else {
                // Partial match — could still complete.
                ValidationResult {
                    outcome: ValidationOutcome::Partial,
                    message: Some("pattern not yet matched".into()),
                }
            }
        } else {
            ValidationResult::error("no regex pattern configured".into())
        }
    }

    fn feed_tag(&mut self, token: &str) -> ValidationResult {
        let parser = self.tag.as_mut().expect("tag parser not initialised");
        let nodes = parser.feed(token);

        let events: Vec<ParserEvent> = nodes
            .iter()
            .map(|node| ParserEvent::TagClose {
                name: node.name.clone(),
            })
            .collect();

        for ev in events {
            self.emit(ev);
        }

        ValidationResult::valid()
    }

    fn emit(&self, event: ParserEvent) {
        for cb in &self.callbacks {
            cb.on_event(&event);
        }
    }

    fn preceding_context(&self) -> String {
        let len = self.raw.len();
        if len <= 20 {
            self.raw.clone()
        } else {
            self.raw[len - 20..].to_string()
        }
    }

    /// Finalize parsing and return the overall validation result.
    pub fn finalize(&mut self) -> ValidationResult {
        match self.mode {
            ParseMode::Json => {
                let validator = self.json.as_ref().expect("JSON validator");
                if validator.is_complete() {
                    ValidationResult::valid()
                } else {
                    ValidationResult::error("incomplete JSON at end of stream".into())
                }
            }
            ParseMode::Xml => {
                let parser = self.xml.as_ref().expect("XML parser");
                if parser.is_valid() {
                    ValidationResult::valid()
                } else {
                    ValidationResult::error("unclosed or malformed XML tags".into())
                }
            }
            ParseMode::Grammar => {
                if let Some(engine) = &self.grammar {
                    if engine.has_failed() {
                        ValidationResult::error("grammar validation failed".into())
                    } else {
                        ValidationResult::valid()
                    }
                } else {
                    ValidationResult::valid()
                }
            }
            ParseMode::Regex => {
                if let Some(pattern) = &self.regex_pattern {
                    if self.raw.contains(pattern.as_str()) {
                        ValidationResult::valid()
                    } else {
                        ValidationResult::error("final output does not contain pattern".into())
                    }
                } else {
                    ValidationResult::valid()
                }
            }
            ParseMode::Tag | ParseMode::Passthrough => ValidationResult::valid(),
        }
    }

    /// Get the current parse mode.
    pub fn mode(&self) -> ParseMode {
        self.mode
    }

    /// Get the accumulated raw output.
    pub fn raw_output(&self) -> &str {
        &self.raw
    }

    /// Reset the parser for a new stream.
    pub fn reset(&mut self) {
        self.raw.clear();
        self.error_count = 0;
        if let Some(v) = &mut self.json {
            v.reset();
        }
        if let Some(p) = &mut self.xml {
            p.reset();
        }
        if let Some(p) = &mut self.tag {
            p.reset();
        }
        if let Some(e) = &mut self.grammar {
            e.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_mode_validates_complete_object() {
        let mut p = StructuredParser::new(ParseMode::Json);
        let r = p.feed(r#"{"key": "value"}"#);
        assert_eq!(r.outcome, ValidationOutcome::Valid);
        let f = p.finalize();
        assert_eq!(f.outcome, ValidationOutcome::Valid);
    }

    #[test]
    fn json_mode_reports_partial() {
        let mut p = StructuredParser::new(ParseMode::Json);
        let r = p.feed(r#"{"key":"#);
        assert_eq!(r.outcome, ValidationOutcome::Partial);
    }

    #[test]
    fn xml_mode_validates_nested_tags() {
        let mut p = StructuredParser::new(ParseMode::Xml);
        let r = p.feed("<a><b>text</b></a>");
        assert_eq!(r.outcome, ValidationOutcome::Valid);
    }

    #[test]
    fn tag_mode_parses_streaming() {
        let mut p = StructuredParser::new(ParseMode::Tag);
        let r1 = p.feed("<tool>");
        assert_eq!(r1.outcome, ValidationOutcome::Valid);
        let r2 = p.feed("data</tool>");
        assert_eq!(r2.outcome, ValidationOutcome::Valid);
    }

    #[test]
    fn passthrough_mode_always_valid() {
        let mut p = StructuredParser::new(ParseMode::Passthrough);
        let r = p.feed("anything goes here");
        assert_eq!(r.outcome, ValidationOutcome::Valid);
    }

    #[test]
    fn parse_mode_from_env_defaults() {
        assert_eq!(
            ParseMode::from_env_or(None, ParseMode::Passthrough),
            ParseMode::Passthrough
        );
        assert_eq!(
            ParseMode::from_env_or(Some("json"), ParseMode::Passthrough),
            ParseMode::Json
        );
    }

    #[test]
    fn reset_clears_parser_state() {
        let mut p = StructuredParser::new(ParseMode::Json);
        p.feed(r#"{"a":1"#);
        p.reset();
        assert_eq!(p.raw_output(), "");
    }
}
