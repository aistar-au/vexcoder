//! XML/tag tree parser with nested tag support and stack-based validation.
//!
//! Unlike the existing flat tag detection in `tool_call_parser.rs`, this
//! module builds a proper tag tree, validates nesting, and supports
//! tag-aware streaming with structural guarantees.

use std::fmt;

/// A node in the parsed tag tree.
#[derive(Debug, Clone, PartialEq)]
pub struct TagNode {
    /// Tag name (e.g. `"tool"`, `"think"`, `"answer"`).
    pub name: String,
    /// Attributes as key-value pairs.
    pub attributes: Vec<(String, String)>,
    /// Child content — interleaved text and nested tags.
    pub children: Vec<TagContent>,
    /// Whether this tag has been properly closed.
    pub closed: bool,
}

/// Content within a tag: either text or a nested tag.
#[derive(Debug, Clone, PartialEq)]
pub enum TagContent {
    Text(String),
    Element(TagNode),
}

impl TagNode {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            attributes: Vec::new(),
            children: Vec::new(),
            closed: false,
        }
    }

    /// Get text content, concatenating all direct text children.
    pub fn text_content(&self) -> String {
        self.children
            .iter()
            .filter_map(|c| match c {
                TagContent::Text(t) => Some(t.as_str()),
                TagContent::Element(_) => None,
            })
            .collect()
    }

    /// Get a specific attribute value.
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Find all direct child elements with the given tag name.
    pub fn children_by_name(&self, name: &str) -> Vec<&TagNode> {
        self.children
            .iter()
            .filter_map(|c| match c {
                TagContent::Element(node) if node.name == name => Some(node),
                _ => None,
            })
            .collect()
    }
}

impl fmt::Display for TagNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}", self.name)?;
        for (k, v) in &self.attributes {
            write!(f, " {k}=\"{v}\"")?;
        }
        if self.children.is_empty() && self.closed {
            write!(f, " />")
        } else {
            write!(f, ">")?;
            for child in &self.children {
                match child {
                    TagContent::Text(t) => write!(f, "{t}")?,
                    TagContent::Element(e) => write!(f, "{e}")?,
                }
            }
            if self.closed {
                write!(f, "</{}>", self.name)?;
            }
            Ok(())
        }
    }
}

/// Stack-based tag validator for streaming input.
///
/// Tracks open tags and validates proper nesting as tokens arrive.
pub struct TagStack {
    /// Stack of currently open tag names.
    stack: Vec<String>,
    /// Validation errors encountered.
    errors: Vec<TagError>,
}

/// A tag nesting or structure error.
#[derive(Debug, Clone)]
pub struct TagError {
    pub message: String,
    pub offset: usize,
}

impl fmt::Display for TagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "offset {}: {}", self.offset, self.message)
    }
}

impl TagStack {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Push an opening tag onto the stack.
    pub fn open(&mut self, name: &str) {
        self.stack.push(name.to_string());
    }

    /// Attempt to close a tag. Returns `true` if the close was valid.
    pub fn close(&mut self, name: &str, offset: usize) -> bool {
        if let Some(top) = self.stack.last() {
            if top == name {
                self.stack.pop();
                return true;
            }
            self.errors.push(TagError {
                message: format!("mismatched close tag: expected </{top}>, got </{name}>"),
                offset,
            });
            // Attempt recovery: search stack for matching open tag.
            if let Some(pos) = self.stack.iter().rposition(|t| t == name) {
                // Close everything above the matching tag.
                self.stack.truncate(pos);
                return true;
            }
            false
        } else {
            self.errors.push(TagError {
                message: format!("close tag </{name}> with no open tag"),
                offset,
            });
            false
        }
    }

    /// Returns the current nesting depth.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Returns the name of the innermost open tag.
    pub fn current_tag(&self) -> Option<&str> {
        self.stack.last().map(String::as_str)
    }

    /// Returns all accumulated errors.
    pub fn errors(&self) -> &[TagError] {
        &self.errors
    }

    /// Returns `true` if all tags are properly closed.
    pub fn is_balanced(&self) -> bool {
        self.stack.is_empty()
    }

    /// Returns the names of all currently open tags (outermost first).
    pub fn open_tags(&self) -> &[String] {
        &self.stack
    }
}

impl Default for TagStack {
    fn default() -> Self {
        Self::new()
    }
}

/// Streaming tag tree parser.
///
/// Processes input incrementally and builds a tag tree while tracking
/// nesting validity via [`TagStack`].
pub struct TagTreeParser {
    /// Accumulated unparsed input.
    buffer: String,
    /// The tag stack for nesting validation.
    stack: TagStack,
    /// Completed top-level nodes.
    roots: Vec<TagNode>,
    /// Working stack of partially-built nodes.
    build_stack: Vec<TagNode>,
    /// Total bytes consumed.
    offset: usize,
}

impl TagTreeParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            stack: TagStack::new(),
            roots: Vec::new(),
            build_stack: Vec::new(),
            offset: 0,
        }
    }

    /// Feed a token of input text.
    ///
    /// Returns any newly completed top-level tag nodes.
    pub fn feed(&mut self, token: &str) -> Vec<TagNode> {
        self.buffer.push_str(token);
        self.offset += token.len();
        self.drain_buffer()
    }

    fn drain_buffer(&mut self) -> Vec<TagNode> {
        let mut completed = Vec::new();

        loop {
            let buf = self.buffer.clone();
            let trimmed = buf.trim_start();

            if trimmed.is_empty() {
                break;
            }

            // Try to find a tag start.
            if let Some(tag_start) = trimmed.find('<') {
                // Any text before the tag is content.
                if tag_start > 0 {
                    let text = trimmed[..tag_start].to_string();
                    self.push_text(&text);
                    self.buffer = trimmed[tag_start..].to_string();
                    continue;
                }

                // Check for closing tag.
                if trimmed.starts_with("</") {
                    if let Some(end) = trimmed.find('>') {
                        let tag_name = trimmed[2..end].trim().to_string();
                        self.buffer = trimmed[end + 1..].to_string();
                        self.stack.close(&tag_name, self.offset);

                        if let Some(mut node) = self.build_stack.pop() {
                            node.closed = true;
                            if self.build_stack.is_empty() {
                                completed.push(node);
                            } else if let Some(parent) = self.build_stack.last_mut() {
                                parent.children.push(TagContent::Element(node));
                            }
                        }
                        continue;
                    }
                    // Incomplete close tag — wait for more input.
                    break;
                }

                // Check for self-closing tag.
                if let Some(end) = trimmed.find('>') {
                    let tag_content = &trimmed[1..end];
                    let is_self_closing = tag_content.ends_with('/');
                    let tag_content = if is_self_closing {
                        &tag_content[..tag_content.len() - 1]
                    } else {
                        tag_content
                    };

                    let (name, attrs) = Self::parse_tag_opener(tag_content);
                    self.buffer = trimmed[end + 1..].to_string();

                    let mut node = TagNode::new(name.clone());
                    node.attributes = attrs;

                    if is_self_closing {
                        node.closed = true;
                        if self.build_stack.is_empty() {
                            completed.push(node);
                        } else if let Some(parent) = self.build_stack.last_mut() {
                            parent.children.push(TagContent::Element(node));
                        }
                    } else {
                        self.stack.open(&name);
                        self.build_stack.push(node);
                    }
                    continue;
                }

                // Incomplete opening tag — wait for more input.
                break;
            } else {
                // No tag found — all remaining content is text.
                let text = trimmed.to_string();
                self.push_text(&text);
                self.buffer.clear();
                break;
            }
        }

        self.roots.extend(completed.iter().cloned());
        completed
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(parent) = self.build_stack.last_mut() {
            parent.children.push(TagContent::Text(text.to_string()));
        }
    }

    fn parse_tag_opener(content: &str) -> (String, Vec<(String, String)>) {
        let content = content.trim();
        let mut parts = content.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_string();
        let attrs_str = parts.next().unwrap_or("");

        let mut attrs = Vec::new();
        let mut remaining = attrs_str.trim();

        while !remaining.is_empty() {
            // Parse: key="value" or key='value'
            if let Some(eq_pos) = remaining.find('=') {
                let key = remaining[..eq_pos].trim().to_string();
                let after_eq = remaining[eq_pos + 1..].trim();

                if after_eq.starts_with('"') || after_eq.starts_with('\'') {
                    let quote = after_eq.as_bytes()[0] as char;
                    if let Some(end_quote) = after_eq[1..].find(quote) {
                        let value = after_eq[1..=end_quote].to_string();
                        attrs.push((key, value));
                        remaining = after_eq[end_quote + 2..].trim();
                        continue;
                    }
                }

                // Unquoted or malformed — take until whitespace.
                let value_end = after_eq.find(char::is_whitespace).unwrap_or(after_eq.len());
                let value = after_eq[..value_end].to_string();
                attrs.push((key, value));
                remaining = after_eq[value_end..].trim();
            } else {
                break;
            }
        }

        (name, attrs)
    }

    /// Returns all completed root nodes.
    pub fn roots(&self) -> &[TagNode] {
        &self.roots
    }

    /// Returns a reference to the tag stack for nesting inspection.
    pub fn tag_stack(&self) -> &TagStack {
        &self.stack
    }

    /// Returns `true` if all tags are properly nested and closed.
    pub fn is_valid(&self) -> bool {
        self.stack.is_balanced() && self.stack.errors().is_empty()
    }

    /// Reset the parser for a new document.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.stack = TagStack::new();
        self.roots.clear();
        self.build_stack.clear();
        self.offset = 0;
    }
}

impl Default for TagTreeParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_tag() {
        let mut p = TagTreeParser::new();
        let nodes = p.feed("<tool>content</tool>");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "tool");
        assert_eq!(nodes[0].text_content(), "content");
        assert!(nodes[0].closed);
    }

    #[test]
    fn parses_nested_tags() {
        let mut p = TagTreeParser::new();
        let nodes = p.feed("<outer><inner>text</inner></outer>");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "outer");
        let inner = nodes[0].children_by_name("inner");
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0].text_content(), "text");
    }

    #[test]
    fn parses_self_closing_tag() {
        let mut p = TagTreeParser::new();
        let nodes = p.feed("<br/>");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "br");
        assert!(nodes[0].closed);
    }

    #[test]
    fn parses_tag_with_attributes() {
        let mut p = TagTreeParser::new();
        let nodes = p.feed(r#"<tool name="read_file" path="/src">data</tool>"#);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].attr("name"), Some("read_file"));
        assert_eq!(nodes[0].attr("path"), Some("/src"));
    }

    #[test]
    fn streaming_incremental_parse() {
        let mut p = TagTreeParser::new();
        assert!(p.feed("<too").is_empty());
        assert!(p.feed("l>con").is_empty());
        assert!(p.feed("tent</t").is_empty());
        let nodes = p.feed("ool>");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].text_content(), "content");
    }

    #[test]
    fn tag_stack_validates_nesting() {
        let mut stack = TagStack::new();
        stack.open("outer");
        stack.open("inner");
        assert!(stack.close("inner", 0));
        assert!(stack.close("outer", 0));
        assert!(stack.is_balanced());
    }

    #[test]
    fn tag_stack_detects_mismatch() {
        let mut stack = TagStack::new();
        stack.open("a");
        stack.open("b");
        // Closing "a" when "b" is on top is a mismatch.  Recovery finds "a"
        // deeper in the stack and truncates, so close returns true, but an
        // error is still recorded.
        assert!(stack.close("a", 10));
        assert!(!stack.errors().is_empty());
    }

    #[test]
    fn multiple_root_elements() {
        let mut p = TagTreeParser::new();
        let n1 = p.feed("<a>1</a>");
        let n2 = p.feed("<b>2</b>");
        assert_eq!(n1.len(), 1);
        assert_eq!(n2.len(), 1);
        assert_eq!(p.roots().len(), 2);
    }

    #[test]
    fn display_roundtrip() {
        let mut node = TagNode::new("tool");
        node.attributes.push(("name".into(), "test".into()));
        node.children.push(TagContent::Text("content".into()));
        node.closed = true;
        let rendered = format!("{node}");
        assert!(rendered.contains("<tool"));
        assert!(rendered.contains("</tool>"));
    }
}
