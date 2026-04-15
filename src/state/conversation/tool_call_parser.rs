use quick_xml::Reader as XmlReader;
/// Pluggable tool-call parsers for normalizing model output into
/// [`TaggedToolCall`] before the orchestrator processes them.
///
/// The default fast path (`Tagged`) uses zero-cost substring scanning for
/// `<function=…>` / `<parameter=…>` tags emitted by most local servers.
/// The optional `Hybrid` mode appends a quick-xml fallback that catches
/// generic XML tool-call formats (`<tool_call>`, `<invoke>`, etc.) when
/// the tagged scan finds nothing.  Hybrid mode is off by default and must
/// be opted into via model profile or environment variable.
use quick_xml::events::Event as XmlEvent;

use super::tools::{TaggedToolCall, parse_tagged_tool_calls};

// ── Configuration ────────────────────────────────────────────────────

/// Selects which parser chain to use for text-protocol tool calls.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolParserMode {
    /// Tagged-only (existing fast path). Default.
    #[default]
    Tagged,
    /// Tagged first, then quick-xml fallback on miss.
    Hybrid,
}

impl ToolParserMode {
    /// Resolve mode from the environment (`VEX_TOOL_PARSER`), then the
    /// optional profile name, falling back to the supplied default.
    pub(crate) fn from_env_or(profile_name: Option<&str>, default: Self) -> Self {
        match std::env::var("VEX_TOOL_PARSER").ok().as_deref() {
            Some("hybrid") => Self::Hybrid,
            Some("tagged") => Self::Tagged,
            _ => match profile_name {
                Some("hybrid") => Self::Hybrid,
                Some("tagged") => Self::Tagged,
                _ => default,
            },
        }
    }
}

// ── Trait ─────────────────────────────────────────────────────────────

pub(crate) trait ToolCallParser: Send + Sync {
    fn parse(&self, text: &str) -> Vec<TaggedToolCall>;
}

// ── Tagged (fast path, unchanged logic) ──────────────────────────────

pub(crate) struct TaggedParser;

impl ToolCallParser for TaggedParser {
    fn parse(&self, text: &str) -> Vec<TaggedToolCall> {
        parse_tagged_tool_calls(text)
    }
}

// ── XML fallback via quick-xml ───────────────────────────────────────

pub(crate) struct XmlFallbackParser;

impl XmlFallbackParser {
    /// Try to parse tool calls from generic XML patterns:
    ///   `<tool_call>…</tool_call>`  or  `<invoke name="…">…</invoke>`
    ///
    /// Returns an empty vec when no recognisable XML tool pattern is found.
    fn parse_xml_tool_calls(text: &str) -> Vec<TaggedToolCall> {
        let mut calls = Vec::new();

        // Scan for `<tool_call>` blocks — the most common generic XML
        // format emitted by local models.
        let mut cursor = 0usize;
        while let Some(start_rel) = text[cursor..].find("<tool_call>") {
            let body_start = cursor + start_rel + "<tool_call>".len();
            let body_end = text[body_start..]
                .find("</tool_call>")
                .map(|r| body_start + r)
                .unwrap_or(text.len());
            let body = &text[body_start..body_end];

            if let Some(call) = Self::parse_tool_call_body(body) {
                calls.push(call);
            }
            cursor = if body_end < text.len() {
                body_end + "</tool_call>".len()
            } else {
                text.len()
            };
        }

        // If no `<tool_call>` blocks found, try `<invoke>` pattern.
        if calls.is_empty() {
            calls = Self::parse_invoke_blocks(text);
        }

        calls
    }

    /// Parse JSON content inside a `<tool_call>` block.
    /// Expects `{"name": "…", "arguments": {…}}` (or `"parameters"`).
    fn parse_tool_call_body(body: &str) -> Option<TaggedToolCall> {
        let trimmed = body.trim();
        let obj: serde_json::Value = serde_json::from_str(trimmed).ok()?;
        let name = obj.get("name").and_then(|v| v.as_str()).map(String::from)?;
        let input = obj
            .get("arguments")
            .or_else(|| obj.get("parameters"))
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        if name.is_empty() {
            return None;
        }
        Some(TaggedToolCall { name, input })
    }

    /// Parse `<invoke name="tool_name">` blocks with child `<parameter>` or
    /// `<argument>` elements, using quick-xml for structured extraction.
    fn parse_invoke_blocks(text: &str) -> Vec<TaggedToolCall> {
        let mut calls = Vec::new();
        let mut reader = XmlReader::from_str(text);
        reader.config_mut().trim_text(true);

        let mut current_name: Option<String> = None;
        let mut current_params = serde_json::Map::new();
        let mut current_key: Option<String> = None;
        let mut depth = 0u32;
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(XmlEvent::Start(ref e)) => match e.name().as_ref() {
                    b"invoke" | b"tool_use" => {
                        depth += 1;
                        current_name = e
                            .attributes()
                            .filter_map(Result::ok)
                            .find(|a| a.key.as_ref() == b"name")
                            .and_then(|a| String::from_utf8(a.value.to_vec()).ok());
                        current_params = serde_json::Map::new();
                    }
                    b"parameter" | b"argument" if depth > 0 => {
                        current_key = e
                            .attributes()
                            .filter_map(Result::ok)
                            .find(|a| a.key.as_ref() == b"name" || a.key.as_ref() == b"key")
                            .and_then(|a| String::from_utf8(a.value.to_vec()).ok());
                    }
                    _ => {}
                },
                Ok(XmlEvent::Text(ref e)) if depth > 0 && current_key.is_some() => {
                    if let (Some(key), Ok(val)) = (current_key.take(), e.decode()) {
                        current_params.insert(key, serde_json::Value::String(val.into_owned()));
                    }
                }
                Ok(XmlEvent::End(ref e)) => match e.name().as_ref() {
                    b"invoke" | b"tool_use" if depth > 0 => {
                        depth -= 1;
                        if let Some(name) = current_name.take()
                            && !name.is_empty()
                        {
                            calls.push(TaggedToolCall {
                                name,
                                input: serde_json::Value::Object(std::mem::take(
                                    &mut current_params,
                                )),
                            });
                        }
                    }
                    _ => {
                        current_key = None;
                    }
                },
                Ok(XmlEvent::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }

        calls
    }
}

impl ToolCallParser for XmlFallbackParser {
    fn parse(&self, text: &str) -> Vec<TaggedToolCall> {
        Self::parse_xml_tool_calls(text)
    }
}

// ── Hybrid parser (tagged → xml fallback) ────────────────────────────

pub(crate) struct HybridParser;

impl ToolCallParser for HybridParser {
    fn parse(&self, text: &str) -> Vec<TaggedToolCall> {
        let tagged = parse_tagged_tool_calls(text);
        if !tagged.is_empty() {
            return tagged;
        }
        XmlFallbackParser::parse_xml_tool_calls(text)
    }
}

// ── Factory ──────────────────────────────────────────────────────────

/// Return the appropriate parser for the given mode.
pub(crate) fn parser_for_mode(mode: ToolParserMode) -> Box<dyn ToolCallParser> {
    match mode {
        ToolParserMode::Tagged => Box::new(TaggedParser),
        ToolParserMode::Hybrid => Box::new(HybridParser),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_parser_delegates_correctly() {
        let text = r#"<function=read_file>
<parameter=path>main.rs</parameter>
</function>"#;
        let parser = TaggedParser;
        let calls = parser.parse(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].input["path"], "main.rs");
    }

    #[test]
    fn xml_fallback_parses_tool_call_json_block() {
        let text = r#"Some reasoning text.
<tool_call>
{"name": "write_file", "arguments": {"path": "lib.rs", "content": "fn main() {}"}}
</tool_call>"#;
        let parser = XmlFallbackParser;
        let calls = parser.parse(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "write_file");
        assert_eq!(calls[0].input["path"], "lib.rs");
        assert_eq!(calls[0].input["content"], "fn main() {}");
    }

    #[test]
    fn xml_fallback_parses_invoke_blocks() {
        let text = r#"<invoke name="list_files">
<parameter name="path">/src</parameter>
</invoke>"#;
        let parser = XmlFallbackParser;
        let calls = parser.parse(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "list_files");
        assert_eq!(calls[0].input["path"], "/src");
    }

    #[test]
    fn xml_fallback_returns_empty_on_no_xml() {
        let text = "Just some plain text with no tool calls.";
        let parser = XmlFallbackParser;
        let calls = parser.parse(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn hybrid_prefers_tagged_over_xml() {
        let text = r#"<function=read_file>
<parameter=path>main.rs</parameter>
</function>"#;
        let parser = HybridParser;
        let calls = parser.parse(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn hybrid_falls_back_to_xml_on_tagged_miss() {
        let text = r#"<tool_call>
{"name": "search", "arguments": {"query": "todo"}}
</tool_call>"#;
        let parser = HybridParser;
        let calls = parser.parse(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "search");
        assert_eq!(calls[0].input["query"], "todo");
    }

    #[test]
    fn xml_fallback_handles_parameters_key() {
        let text = r#"<tool_call>
{"name": "edit", "parameters": {"file": "a.rs"}}
</tool_call>"#;
        let calls = XmlFallbackParser::parse_xml_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].input["file"], "a.rs");
    }

    #[test]
    fn xml_fallback_handles_multiple_tool_calls() {
        let text = r#"<tool_call>
{"name": "read_file", "arguments": {"path": "a.rs"}}
</tool_call>
<tool_call>
{"name": "read_file", "arguments": {"path": "b.rs"}}
</tool_call>"#;
        let calls = XmlFallbackParser::parse_xml_tool_calls(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].input["path"], "a.rs");
        assert_eq!(calls[1].input["path"], "b.rs");
    }

    #[test]
    fn parser_for_mode_returns_correct_type() {
        let tagged = parser_for_mode(ToolParserMode::Tagged);
        let hybrid = parser_for_mode(ToolParserMode::Hybrid);

        let text = r#"<tool_call>
{"name": "test", "arguments": {}}
</tool_call>"#;

        // Tagged parser should not find tool_call blocks.
        assert!(tagged.parse(text).is_empty());
        // Hybrid parser should find them via fallback.
        assert_eq!(hybrid.parse(text).len(), 1);
    }

    #[test]
    fn xml_fallback_rejects_empty_name() {
        let text = r#"<tool_call>
{"name": "", "arguments": {"path": "a.rs"}}
</tool_call>"#;
        let calls = XmlFallbackParser::parse_xml_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn xml_fallback_handles_malformed_json_gracefully() {
        let text = r#"<tool_call>
not valid json at all
</tool_call>"#;
        let calls = XmlFallbackParser::parse_xml_tool_calls(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn xml_fallback_handles_incomplete_block() {
        let text = r#"<tool_call>
{"name": "read_file", "arguments": {"path": "a.rs"}}"#;
        let calls = XmlFallbackParser::parse_xml_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn from_env_or_respects_profile_name() {
        assert_eq!(
            ToolParserMode::from_env_or(Some("hybrid"), ToolParserMode::Tagged),
            ToolParserMode::Hybrid
        );
        assert_eq!(
            ToolParserMode::from_env_or(Some("tagged"), ToolParserMode::Tagged),
            ToolParserMode::Tagged
        );
        assert_eq!(
            ToolParserMode::from_env_or(Some("unknown"), ToolParserMode::Tagged),
            ToolParserMode::Tagged
        );
        assert_eq!(
            ToolParserMode::from_env_or(None, ToolParserMode::Tagged),
            ToolParserMode::Tagged
        );
    }

    // ── ADR-043 gate 2: parity fixtures ─────────────────────────────

    #[test]
    fn tagged_and_hybrid_agree_on_well_formed_tagged_input() {
        let text = r#"<function=read_file>
<parameter=path>src/main.rs</parameter>
</function>"#;
        let tagged = TaggedParser.parse(text);
        let hybrid = HybridParser.parse(text);
        assert_eq!(tagged.len(), hybrid.len(), "parsers should agree on count");
        for (t, h) in tagged.iter().zip(hybrid.iter()) {
            assert_eq!(t.name, h.name, "tool name mismatch");
            assert_eq!(t.input, h.input, "tool input mismatch");
        }
    }

    #[test]
    fn tagged_and_hybrid_agree_on_multiple_tagged_calls() {
        let text = r#"<function=read_file>
<parameter=path>a.rs</parameter>
</function>
<function=write_file>
<parameter=path>b.rs</parameter>
<parameter=content>fn main() {}</parameter>
</function>"#;
        let tagged = TaggedParser.parse(text);
        let hybrid = HybridParser.parse(text);
        assert_eq!(tagged.len(), 2);
        assert_eq!(hybrid.len(), 2);
        assert_eq!(tagged[0].name, hybrid[0].name);
        assert_eq!(tagged[1].name, hybrid[1].name);
        assert_eq!(tagged[0].input, hybrid[0].input);
        assert_eq!(tagged[1].input, hybrid[1].input);
    }

    #[test]
    fn xml_fallback_recovers_from_malformed_json_between_valid_blocks() {
        let text = r#"<tool_call>
not json
</tool_call>
<tool_call>
{"name": "read_file", "arguments": {"path": "ok.rs"}}
</tool_call>"#;
        let calls = XmlFallbackParser::parse_xml_tool_calls(text);
        assert_eq!(
            calls.len(),
            1,
            "should skip the broken block and parse the valid one"
        );
        assert_eq!(calls[0].name, "read_file");
    }

    #[test]
    fn xml_fallback_handles_nested_json_in_arguments() {
        let text = r#"<tool_call>
{"name": "edit_file", "arguments": {"path": "config.json", "content": "{\"key\": \"value\"}"}}
</tool_call>"#;
        let calls = XmlFallbackParser::parse_xml_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "edit_file");
        assert!(
            calls[0].input["content"].as_str().unwrap().contains("key"),
            "nested JSON in arguments should be preserved"
        );
    }

    #[test]
    fn tagged_parser_ignores_xml_format() {
        // Tagged parser intentionally does not parse <tool_call> or <invoke>
        // format — that is the domain of hybrid/xml fallback only.
        let text = r#"<tool_call>
{"name": "test", "arguments": {}}
</tool_call>"#;
        let calls = TaggedParser.parse(text);
        assert!(
            calls.is_empty(),
            "tagged parser should not match XML format"
        );
    }

    #[test]
    fn hybrid_parser_returns_empty_on_plain_text() {
        let text = "Just a normal response with no tool calls at all.";
        let calls = HybridParser.parse(text);
        assert!(calls.is_empty());
    }

    #[test]
    fn xml_fallback_invoke_with_tool_use_tag() {
        let text = r#"<tool_use name="search_files">
<parameter name="query">TODO</parameter>
<parameter name="path">src/</parameter>
</tool_use>"#;
        let calls = XmlFallbackParser::parse_xml_tool_calls(text);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "search_files");
        assert_eq!(calls[0].input["query"], "TODO");
        assert_eq!(calls[0].input["path"], "src/");
    }
}
