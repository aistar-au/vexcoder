//! Incremental JSON streaming validator with partial-token recovery.
//!
//! Validates JSON as it streams token-by-token, maintaining a state machine
//! that tracks nesting depth, current context (object/array/string/value),
//! and can recover from common malformations.

use std::fmt;

/// The current validation state of the JSON stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonValidationState {
    /// No input received yet.
    Empty,
    /// Inside a valid JSON prefix; parsing can continue.
    Partial(JsonContext),
    /// The accumulated input is a complete, valid JSON value.
    Complete,
    /// A structural error was detected at the given byte offset.
    Error { offset: usize, message: String },
    /// Error was detected but recovery was attempted.
    Recovered { offset: usize, message: String },
}

/// The innermost JSON context the validator is currently inside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonContext {
    TopLevel,
    Object { depth: usize },
    Array { depth: usize },
    String,
    Number,
    Keyword,
}

impl fmt::Display for JsonContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TopLevel => write!(f, "top-level"),
            Self::Object { depth } => write!(f, "object (depth {depth})"),
            Self::Array { depth } => write!(f, "array (depth {depth})"),
            Self::String => write!(f, "string"),
            Self::Number => write!(f, "number"),
            Self::Keyword => write!(f, "keyword"),
        }
    }
}

/// Streaming JSON validator that processes input incrementally.
///
/// Feed tokens (string fragments) via [`feed`](Self::feed). The validator
/// maintains enough state to determine whether the accumulated input is a
/// valid JSON prefix, a complete value, or contains a structural error.
pub struct JsonStreamValidator {
    /// Nesting stack: `{` pushes Object, `[` pushes Array.
    stack: Vec<StackFrame>,
    /// Whether we are currently inside a JSON string.
    in_string: bool,
    /// Whether the previous character was a backslash (escape).
    escape_next: bool,
    /// Total bytes processed.
    offset: usize,
    /// Whether a complete top-level value has been closed.
    complete: bool,
    /// Whether strict mode rejects trailing content after the first value.
    strict: bool,
    /// Recovery log for error reporting.
    recoveries: Vec<(usize, String)>,
    /// Accumulated raw input for re-parse on recovery.
    raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StackFrame {
    Object,
    Array,
}

impl JsonStreamValidator {
    /// Create a new validator.
    ///
    /// When `strict` is `true`, the validator rejects any tokens after the
    /// first complete top-level JSON value.
    pub fn new(strict: bool) -> Self {
        Self {
            stack: Vec::new(),
            in_string: false,
            escape_next: false,
            offset: 0,
            complete: false,
            strict,
            recoveries: Vec::new(),
            raw: String::new(),
        }
    }

    /// Feed a token (string fragment) into the validator.
    ///
    /// Returns the validation state after processing this token.
    pub fn feed(&mut self, token: &str) -> JsonValidationState {
        self.raw.push_str(token);

        for byte in token.bytes() {
            let ch = byte as char;
            self.offset += 1;

            if self.complete && self.strict {
                // After a complete value in strict mode, only whitespace is allowed.
                if !ch.is_ascii_whitespace() {
                    return JsonValidationState::Error {
                        offset: self.offset,
                        message: format!("trailing content after complete JSON value: '{ch}'"),
                    };
                }
                continue;
            }

            if self.in_string {
                if self.escape_next {
                    self.escape_next = false;
                    continue;
                }
                match ch {
                    '\\' => self.escape_next = true,
                    '"' => {
                        self.in_string = false;
                        if self.stack.is_empty() {
                            self.complete = true;
                        }
                    }
                    _ => {}
                }
                continue;
            }

            // Outside a string.
            match ch {
                ' ' | '\t' | '\n' | '\r' => continue,
                '"' => {
                    self.in_string = true;
                    self.escape_next = false;
                }
                '{' => self.stack.push(StackFrame::Object),
                '[' => self.stack.push(StackFrame::Array),
                '}' => {
                    if self.stack.last() == Some(&StackFrame::Object) {
                        self.stack.pop();
                        if self.stack.is_empty() {
                            self.complete = true;
                        }
                    } else {
                        return self.try_recover(format!(
                            "unexpected '}}' at offset {} (not inside object)",
                            self.offset
                        ));
                    }
                }
                ']' => {
                    if self.stack.last() == Some(&StackFrame::Array) {
                        self.stack.pop();
                        if self.stack.is_empty() {
                            self.complete = true;
                        }
                    } else {
                        return self.try_recover(format!(
                            "unexpected ']' at offset {} (not inside array)",
                            self.offset
                        ));
                    }
                }
                ':' | ',' => {
                    // Structural characters; valid inside objects/arrays.
                    if self.stack.is_empty() {
                        return self.try_recover(format!(
                            "unexpected '{ch}' at offset {} outside container",
                            self.offset
                        ));
                    }
                }
                // Start of number, boolean, or null.
                '0'..='9' | '-' | 't' | 'f' | 'n' => {}
                // Continuation characters for numbers, booleans, null.
                '.' | 'e' | 'E' | '+' | 'a'..='z' => {}
                _ => {
                    return self.try_recover(format!(
                        "unexpected character '{ch}' at offset {}",
                        self.offset
                    ));
                }
            }
        }

        self.current_state()
    }

    fn try_recover(&mut self, message: String) -> JsonValidationState {
        let offset = self.offset;
        self.recoveries.push((offset, message.clone()));

        // Recovery strategy: skip the offending character and continue.
        if self.recoveries.len() <= 3 {
            JsonValidationState::Recovered { offset, message }
        } else {
            JsonValidationState::Error { offset, message }
        }
    }

    /// Current state without consuming new input.
    pub fn current_state(&self) -> JsonValidationState {
        if self.complete {
            return JsonValidationState::Complete;
        }
        if self.raw.is_empty() {
            return JsonValidationState::Empty;
        }

        let context = if self.in_string {
            JsonContext::String
        } else {
            match self.stack.last() {
                Some(StackFrame::Object) => JsonContext::Object {
                    depth: self.stack.len(),
                },
                Some(StackFrame::Array) => JsonContext::Array {
                    depth: self.stack.len(),
                },
                None => JsonContext::TopLevel,
            }
        };

        JsonValidationState::Partial(context)
    }

    /// Returns the nesting depth (number of open containers).
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Returns `true` if a complete top-level value has been parsed.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Returns the total bytes processed.
    pub fn bytes_processed(&self) -> usize {
        self.offset
    }

    /// Returns any recovery events that occurred.
    pub fn recoveries(&self) -> &[(usize, String)] {
        &self.recoveries
    }

    /// Attempt to extract a valid JSON value from the accumulated input,
    /// even if the stream is incomplete (best-effort closing of open
    /// containers).
    pub fn best_effort_value(&self) -> Option<serde_json::Value> {
        let mut attempt = self.raw.clone();

        // Close any open strings.
        if self.in_string {
            attempt.push('"');
        }

        // Close open containers in reverse order.
        for frame in self.stack.iter().rev() {
            match frame {
                StackFrame::Object => attempt.push('}'),
                StackFrame::Array => attempt.push(']'),
            }
        }

        serde_json::from_str(&attempt).ok()
    }

    /// Reset the validator for a new stream.
    pub fn reset(&mut self) {
        self.stack.clear();
        self.in_string = false;
        self.escape_next = false;
        self.offset = 0;
        self.complete = false;
        self.recoveries.clear();
        self.raw.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_complete_json_object() {
        let mut v = JsonStreamValidator::new(true);
        let state = v.feed(r#"{"key": "value"}"#);
        assert_eq!(state, JsonValidationState::Complete);
    }

    #[test]
    fn validates_partial_json_object() {
        let mut v = JsonStreamValidator::new(true);
        let state = v.feed(r#"{"key": "#);
        assert!(matches!(state, JsonValidationState::Partial(JsonContext::Object { depth: 1 })));
    }

    #[test]
    fn validates_streaming_tokens() {
        let mut v = JsonStreamValidator::new(true);
        assert!(matches!(v.feed("{"), JsonValidationState::Partial(_)));
        assert!(matches!(v.feed("\"k\""), JsonValidationState::Partial(_)));
        assert!(matches!(v.feed(":"), JsonValidationState::Partial(_)));
        assert!(matches!(v.feed("1"), JsonValidationState::Partial(_)));
        assert_eq!(v.feed("}"), JsonValidationState::Complete);
    }

    #[test]
    fn rejects_trailing_content_in_strict_mode() {
        let mut v = JsonStreamValidator::new(true);
        v.feed("{}");
        let state = v.feed("x");
        assert!(matches!(state, JsonValidationState::Error { .. }));
    }

    #[test]
    fn allows_trailing_whitespace_in_strict_mode() {
        let mut v = JsonStreamValidator::new(true);
        v.feed("{}");
        let state = v.feed("  \n");
        assert_eq!(state, JsonValidationState::Complete);
    }

    #[test]
    fn recovers_from_mismatched_brace() {
        let mut v = JsonStreamValidator::new(false);
        let state = v.feed("}");
        assert!(matches!(state, JsonValidationState::Recovered { .. }));
    }

    #[test]
    fn best_effort_closes_open_containers() {
        let mut v = JsonStreamValidator::new(false);
        v.feed(r#"{"key": [1, 2"#);
        let val = v.best_effort_value();
        assert!(val.is_some());
        let obj = val.unwrap();
        assert!(obj.get("key").unwrap().is_array());
    }

    #[test]
    fn handles_nested_arrays_and_objects() {
        let mut v = JsonStreamValidator::new(true);
        let state = v.feed(r#"{"a": [{"b": 1}, {"c": [2, 3]}]}"#);
        assert_eq!(state, JsonValidationState::Complete);
        assert_eq!(v.depth(), 0);
    }

    #[test]
    fn handles_escaped_strings() {
        let mut v = JsonStreamValidator::new(true);
        let state = v.feed(r#"{"msg": "hello \"world\""}"#);
        assert_eq!(state, JsonValidationState::Complete);
    }

    #[test]
    fn reset_clears_all_state() {
        let mut v = JsonStreamValidator::new(true);
        v.feed(r#"{"key":"#);
        v.reset();
        assert_eq!(v.bytes_processed(), 0);
        assert_eq!(v.depth(), 0);
        assert!(!v.is_complete());
    }
}
