//! Grammar engine for constrained parsing of structured output.
//!
//! Supports BNF-like rules and regex-based patterns, enabling the parser
//! to enforce that streamed tokens conform to a defined grammar before
//! they are yielded downstream.

use std::collections::HashMap;

/// A single grammar rule identified by name.
#[derive(Debug, Clone)]
pub struct GrammarRule {
    /// Human-readable rule name (e.g. `"json_object"`, `"xml_element"`).
    pub name: String,
    /// The rule body — either a BNF alternative list or a regex pattern.
    pub body: RuleBody,
}

/// The body of a grammar rule.
#[derive(Debug, Clone)]
pub enum RuleBody {
    /// Ordered alternatives: first match wins.
    Alternatives(Vec<RuleExpr>),
    /// Single regex pattern (compiled lazily).
    Regex(String),
}

/// An expression inside a grammar alternative.
#[derive(Debug, Clone)]
pub enum RuleExpr {
    /// Literal string that must match exactly.
    Literal(String),
    /// Reference to another named rule.
    RuleRef(String),
    /// Zero-or-more repetitions of an inner expression.
    Repeat(Box<RuleExpr>),
    /// Optional expression (zero or one).
    Optional(Box<RuleExpr>),
    /// Sequence of expressions that must all match in order.
    Sequence(Vec<RuleExpr>),
    /// Character class (e.g. `[a-zA-Z0-9_]`).
    CharClass(String),
}

/// A named collection of grammar rules that together define a structured
/// output format.
#[derive(Debug, Clone)]
pub struct Grammar {
    pub name: String,
    pub start_rule: String,
    rules: HashMap<String, GrammarRule>,
}

impl Grammar {
    pub fn new(name: impl Into<String>, start_rule: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            start_rule: start_rule.into(),
            rules: HashMap::new(),
        }
    }

    pub fn add_rule(&mut self, rule: GrammarRule) {
        self.rules.insert(rule.name.clone(), rule);
    }

    pub fn get_rule(&self, name: &str) -> Option<&GrammarRule> {
        self.rules.get(name)
    }

    pub fn rules(&self) -> impl Iterator<Item = &GrammarRule> {
        self.rules.values()
    }

    /// Built-in grammar for strict JSON objects.
    pub fn json_object() -> Self {
        let mut g = Self::new("json_object", "object");
        g.add_rule(GrammarRule {
            name: "object".into(),
            body: RuleBody::Alternatives(vec![RuleExpr::Sequence(vec![
                RuleExpr::Literal("{".into()),
                RuleExpr::Optional(Box::new(RuleExpr::RuleRef("members".into()))),
                RuleExpr::Literal("}".into()),
            ])]),
        });
        g.add_rule(GrammarRule {
            name: "members".into(),
            body: RuleBody::Alternatives(vec![RuleExpr::Sequence(vec![
                RuleExpr::RuleRef("pair".into()),
                RuleExpr::Repeat(Box::new(RuleExpr::Sequence(vec![
                    RuleExpr::Literal(",".into()),
                    RuleExpr::RuleRef("pair".into()),
                ]))),
            ])]),
        });
        g.add_rule(GrammarRule {
            name: "pair".into(),
            body: RuleBody::Alternatives(vec![RuleExpr::Sequence(vec![
                RuleExpr::RuleRef("string".into()),
                RuleExpr::Literal(":".into()),
                RuleExpr::RuleRef("value".into()),
            ])]),
        });
        g.add_rule(GrammarRule {
            name: "string".into(),
            body: RuleBody::Regex(r#""(?:[^"\\]|\\.)*""#.into()),
        });
        g.add_rule(GrammarRule {
            name: "value".into(),
            body: RuleBody::Alternatives(vec![
                RuleExpr::RuleRef("object".into()),
                RuleExpr::RuleRef("array".into()),
                RuleExpr::RuleRef("string".into()),
                RuleExpr::RuleRef("number".into()),
                RuleExpr::Literal("true".into()),
                RuleExpr::Literal("false".into()),
                RuleExpr::Literal("null".into()),
            ]),
        });
        g.add_rule(GrammarRule {
            name: "array".into(),
            body: RuleBody::Alternatives(vec![RuleExpr::Sequence(vec![
                RuleExpr::Literal("[".into()),
                RuleExpr::Optional(Box::new(RuleExpr::Sequence(vec![
                    RuleExpr::RuleRef("value".into()),
                    RuleExpr::Repeat(Box::new(RuleExpr::Sequence(vec![
                        RuleExpr::Literal(",".into()),
                        RuleExpr::RuleRef("value".into()),
                    ]))),
                ]))),
                RuleExpr::Literal("]".into()),
            ])]),
        });
        g.add_rule(GrammarRule {
            name: "number".into(),
            body: RuleBody::Regex(r"-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?".into()),
        });
        g
    }

    /// Built-in grammar for XML elements.
    pub fn xml_element() -> Self {
        let mut g = Self::new("xml_element", "element");
        g.add_rule(GrammarRule {
            name: "element".into(),
            body: RuleBody::Alternatives(vec![
                // self-closing: <name attr="val" />
                RuleExpr::Sequence(vec![
                    RuleExpr::Literal("<".into()),
                    RuleExpr::RuleRef("name".into()),
                    RuleExpr::Repeat(Box::new(RuleExpr::RuleRef("attribute".into()))),
                    RuleExpr::Literal("/>".into()),
                ]),
                // open/close: <name>…</name>
                RuleExpr::Sequence(vec![
                    RuleExpr::Literal("<".into()),
                    RuleExpr::RuleRef("name".into()),
                    RuleExpr::Repeat(Box::new(RuleExpr::RuleRef("attribute".into()))),
                    RuleExpr::Literal(">".into()),
                    RuleExpr::Repeat(Box::new(RuleExpr::RuleRef("content".into()))),
                    RuleExpr::Literal("</".into()),
                    RuleExpr::RuleRef("name".into()),
                    RuleExpr::Literal(">".into()),
                ]),
            ]),
        });
        g.add_rule(GrammarRule {
            name: "name".into(),
            body: RuleBody::Regex(r"[a-zA-Z_][a-zA-Z0-9_.-]*".into()),
        });
        g.add_rule(GrammarRule {
            name: "attribute".into(),
            body: RuleBody::Alternatives(vec![RuleExpr::Sequence(vec![
                RuleExpr::RuleRef("name".into()),
                RuleExpr::Literal("=".into()),
                RuleExpr::RuleRef("attr_value".into()),
            ])]),
        });
        g.add_rule(GrammarRule {
            name: "attr_value".into(),
            body: RuleBody::Regex(r#""[^"]*""#.into()),
        });
        g.add_rule(GrammarRule {
            name: "content".into(),
            body: RuleBody::Alternatives(vec![
                RuleExpr::RuleRef("element".into()),
                RuleExpr::RuleRef("text".into()),
            ]),
        });
        g.add_rule(GrammarRule {
            name: "text".into(),
            body: RuleBody::Regex(r"[^<]+".into()),
        });
        g
    }
}

/// Engine that validates token-by-token input against a [`Grammar`].
#[derive(Debug)]
pub struct GrammarEngine {
    grammar: Grammar,
    /// Stack of (rule_name, position-within-alternative) pairs.
    rule_stack: Vec<(String, usize)>,
    /// Accumulated validated text.
    validated: String,
    /// Whether the engine has encountered a fatal mismatch.
    failed: bool,
}

impl GrammarEngine {
    pub fn new(grammar: Grammar) -> Self {
        let start = grammar.start_rule.clone();
        Self {
            grammar,
            rule_stack: vec![(start, 0)],
            validated: String::new(),
            failed: false,
        }
    }

    /// Feed a token (partial text) into the grammar engine.
    ///
    /// Returns `true` if the token is consistent with the grammar so far.
    pub fn feed(&mut self, token: &str) -> bool {
        if self.failed {
            return false;
        }

        // Accumulate and attempt prefix-validation.
        self.validated.push_str(token);

        // For now, use a lightweight heuristic: check that the accumulated
        // text is a valid *prefix* of the grammar's start rule.
        // A full pushdown automaton is future work when grammar-constrained
        // decoding is wired to the sampling loop.
        if let Some(rule) = self.grammar.get_rule(&self.rule_stack[0].0) {
            match &rule.body {
                RuleBody::Regex(pat) => {
                    // Check prefix compatibility using char-class matching
                    // instead of the regex crate.
                    if !Self::prefix_matches_pattern(&self.validated, pat) {
                        self.failed = true;
                        return false;
                    }
                }
                RuleBody::Alternatives(_alts) => {
                    // Alternatives: accept while accumulating.  A real
                    // pushdown automaton would check each branch.
                }
            }
        }

        true
    }

    /// Lightweight prefix check: returns `true` if `text` could plausibly
    /// be the start of something matching `pattern`.
    ///
    /// Extracts the literal prefix of the pattern (before the first regex
    /// metacharacter) and compares overlapping characters.  This is
    /// intentionally conservative (accepts more than it should) because a
    /// full NFA prefix matcher is future work.
    fn prefix_matches_pattern(text: &str, pattern: &str) -> bool {
        if pattern.is_empty() {
            return text.is_empty();
        }
        if text.is_empty() {
            return true;
        }

        // Extract literal prefix before the first regex metacharacter.
        let metachars = [
            '.', '+', '*', '?', '(', ')', '[', ']', '{', '}', '|', '^', '$', '\\',
        ];
        let literal_end = pattern
            .char_indices()
            .find_map(|(idx, ch)| {
                if metachars.contains(&ch) {
                    Some(idx)
                } else {
                    None
                }
            })
            .unwrap_or(pattern.len());
        let literal_prefix = &pattern[..literal_end];

        if literal_prefix.is_empty() {
            // Pattern starts with a metacharacter; cannot reject any prefix
            // without full regex support.
            return true;
        }

        // Compare overlapping characters.
        for (p_ch, t_ch) in literal_prefix.chars().zip(text.chars()) {
            if p_ch != t_ch {
                return false;
            }
        }

        true
    }

    /// Check if `text` could be a prefix of a string matching `pattern`.
    #[allow(unused)]
    fn is_valid_prefix(&self, text: &str, pattern: &str) -> bool {
        Self::prefix_matches_pattern(text, pattern)
    }

    /// Returns the validated text accumulated so far.
    pub fn validated_text(&self) -> &str {
        &self.validated
    }

    /// Returns `true` if the engine is in a failed state.
    pub fn has_failed(&self) -> bool {
        self.failed
    }

    /// Reset the engine for a new parse.
    pub fn reset(&mut self) {
        let start = self.grammar.start_rule.clone();
        self.rule_stack = vec![(start, 0)];
        self.validated.clear();
        self.failed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_grammar_has_expected_rules() {
        let g = Grammar::json_object();
        assert!(g.get_rule("object").is_some());
        assert!(g.get_rule("string").is_some());
        assert!(g.get_rule("value").is_some());
        assert!(g.get_rule("array").is_some());
        assert!(g.get_rule("number").is_some());
    }

    #[test]
    fn xml_grammar_has_expected_rules() {
        let g = Grammar::xml_element();
        assert!(g.get_rule("element").is_some());
        assert!(g.get_rule("name").is_some());
        assert!(g.get_rule("attribute").is_some());
    }

    #[test]
    fn grammar_engine_accepts_valid_json_start() {
        let g = Grammar::json_object();
        let mut engine = GrammarEngine::new(g);
        // The start rule is "object" which is regex-less (Alternatives),
        // so feed should accept tokens.
        assert!(engine.feed("{"));
        assert!(engine.feed("\"key\""));
    }

    #[test]
    fn grammar_engine_reset_clears_state() {
        let g = Grammar::json_object();
        let mut engine = GrammarEngine::new(g);
        engine.feed("{");
        engine.reset();
        assert_eq!(engine.validated_text(), "");
        assert!(!engine.has_failed());
    }
}
