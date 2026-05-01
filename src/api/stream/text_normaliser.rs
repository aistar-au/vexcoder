#[derive(Debug, Clone, PartialEq)]
pub enum NormalisedChunk {
    Text(String),
    TaggedToolCall {
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Default)]
pub struct StreamTextNormaliser {
    pending: String,
}

impl StreamTextNormaliser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn normalise(&mut self, text: &str) -> Vec<NormalisedChunk> {
        if text.is_empty() {
            return Vec::new();
        }

        self.pending.push_str(text);
        let mut chunks = Vec::new();

        loop {
            let function_start = self.pending.find("<function=");
            let hermes_start = find_hermes_json_tool_call(&self.pending);

            let take_hermes = match (function_start, hermes_start) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(fs), Some(hs)) => hs < fs,
            };

            if take_hermes {
                let hs = hermes_start.unwrap();
                let prefix = self.pending[..hs].to_string();
                push_text_chunk(&mut chunks, strip_tool_call_wrappers(&prefix));

                let remaining = self.pending[hs..].to_string();
                let content_start = "<tool_call>".len();
                let Some(close_rel) = remaining.find("</tool_call>") else {
                    self.pending = remaining;
                    break;
                };

                let content = remaining[content_start..close_rel].trim();
                if let Some((name, input)) = parse_hermes_json_tool_call(content) {
                    chunks.push(NormalisedChunk::TaggedToolCall { name, input });
                }

                let close_end = close_rel + "</tool_call>".len();
                self.pending = remaining[close_end..].to_string();
                continue;
            }

            let Some(function_start) = function_start else {
                let (text_prefix, pending_suffix) = split_incomplete_tool_tag_suffix(&self.pending);
                push_text_chunk(&mut chunks, strip_tool_call_wrappers(text_prefix));
                self.pending = pending_suffix.to_string();
                break;
            };

            let prefix = self.pending[..function_start].to_string();
            push_text_chunk(&mut chunks, strip_tool_call_wrappers(&prefix));

            let remaining = self.pending[function_start..].to_string();
            let Some(function_close_rel) = remaining.find("</function>") else {
                self.pending = remaining;
                break;
            };

            let function_end = function_close_rel + "</function>".len();
            let raw_function = &remaining[..function_end];
            if let Some((name, input)) = parse_single_tagged_tool_call(raw_function) {
                chunks.push(NormalisedChunk::TaggedToolCall { name, input });
            } else {
                push_text_chunk(&mut chunks, strip_tool_call_wrappers(raw_function));
            }

            let mut next_cursor = function_end;
            while remaining[next_cursor..].starts_with("</tool_call>") {
                next_cursor += "</tool_call>".len();
            }
            self.pending = remaining[next_cursor..].to_string();
        }

        chunks
    }

    pub fn finish(&mut self) -> Vec<NormalisedChunk> {
        let text = strip_tool_call_wrappers(strip_incomplete_tool_tag_suffix(&self.pending));
        self.pending.clear();
        if text.is_empty() {
            Vec::new()
        } else {
            vec![NormalisedChunk::Text(text)]
        }
    }
}

fn push_text_chunk(chunks: &mut Vec<NormalisedChunk>, text: String) {
    if !text.is_empty() {
        chunks.push(NormalisedChunk::Text(text));
    }
}

fn strip_tool_call_wrappers(text: &str) -> String {
    text.replace("<tool_call>", "").replace("</tool_call>", "")
}

fn find_hermes_json_tool_call(text: &str) -> Option<usize> {
    let tag = "<tool_call>";
    let mut search_from = 0;
    loop {
        let rel = text[search_from..].find(tag)?;
        let abs = search_from + rel;
        let after_tag = &text[abs + tag.len()..];
        if after_tag.trim_start().starts_with('{') {
            return Some(abs);
        }
        search_from = abs + 1;
    }
}

fn parse_hermes_json_tool_call(content: &str) -> Option<(String, serde_json::Value)> {
    let obj = serde_json::from_str::<serde_json::Value>(content).ok()?;
    let name = obj.get("name")?.as_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let input = match obj.get("arguments") {
        Some(serde_json::Value::String(s)) => serde_json::from_str(s)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new())),
        Some(v) => v.clone(),
        None => serde_json::Value::Object(serde_json::Map::new()),
    };
    Some((name, input))
}

fn split_incomplete_tool_tag_suffix(text: &str) -> (&str, &str) {
    let Some(last_open) = text.rfind('<') else {
        return (text, "");
    };

    let suffix = &text[last_open..];
    if looks_like_partial_tool_tag(suffix) {
        (&text[..last_open], suffix)
    } else {
        (text, "")
    }
}

fn strip_incomplete_tool_tag_suffix(text: &str) -> &str {
    let Some(last_open) = text.rfind('<') else {
        return text;
    };

    let suffix = &text[last_open..];
    if looks_like_partial_tool_tag(suffix) {
        &text[..last_open]
    } else {
        text
    }
}

fn looks_like_partial_tool_tag(suffix: &str) -> bool {
    let suffix = suffix.to_ascii_lowercase();
    [
        "<function=",
        "<function",
        "</function>",
        "</function",
        "<parameter=",
        "<parameter",
        "</parameter>",
        "</parameter",
        "<tool_call>",
        "<tool_call",
        "</tool_call>",
        "</tool_call",
    ]
    .iter()
    .any(|candidate| candidate.starts_with(&suffix))
}

fn parse_single_tagged_tool_call(raw: &str) -> Option<(String, serde_json::Value)> {
    let name_start = "<function=".len();
    let name_end_rel = raw[name_start..].find('>')?;
    let name_end = name_start + name_end_rel;
    let name = raw[name_start..name_end]
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    if name.is_empty() {
        return None;
    }

    let body = &raw[name_end + 1..raw.len() - "</function>".len()];

    let params = parse_tagged_parameters(body);
    if !params.is_empty() {
        return Some((name, serde_json::Value::Object(params)));
    }

    let trimmed_body = body.trim();
    if trimmed_body.starts_with('{')
        && let Ok(serde_json::Value::Object(map)) = serde_json::from_str(trimmed_body)
    {
        return Some((name, serde_json::Value::Object(map)));
    }

    Some((name, serde_json::Value::Object(params)))
}

fn parse_tagged_parameters(body: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut input = serde_json::Map::new();
    let mut parameter_cursor = 0usize;

    while let Some(parameter_rel) = body[parameter_cursor..].find("<parameter=") {
        let parameter_start = parameter_cursor + parameter_rel;
        let key_start = parameter_start + "<parameter=".len();
        let Some(key_end_rel) = body[key_start..].find('>') else {
            break;
        };
        let key_end = key_start + key_end_rel;
        let key = body[key_start..key_end]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        let value_start = key_end + 1;
        let parameter_close = body[value_start..]
            .find("</parameter>")
            .map(|rel| value_start + rel);
        let next_parameter = body[value_start..]
            .find("<parameter=")
            .map(|rel| value_start + rel);

        let (value_end, next_cursor) = match (parameter_close, next_parameter) {
            (Some(close), Some(next)) if next < close => (next, next),
            (Some(close), _) => (close, close + "</parameter>".len()),
            (None, Some(next)) => (next, next),
            (None, None) => (body.len(), body.len()),
        };

        if !key.is_empty() {
            input.insert(
                key,
                tagged_parameter_json_value(&body[value_start..value_end]),
            );
        }

        parameter_cursor = next_cursor.max(parameter_start + 1);
    }

    input
}

fn tagged_parameter_json_value(raw: &str) -> serde_json::Value {
    let value = normalize_tagged_parameter_value(raw);
    let trimmed = value.trim();

    if trimmed.is_empty() {
        return serde_json::Value::String(value);
    }

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed)
        && !matches!(parsed, serde_json::Value::String(_))
    {
        return parsed;
    }

    if let Ok(parsed) = trimmed.parse::<u64>() {
        return serde_json::Value::Number(parsed.into());
    }

    if let Ok(parsed) = trimmed.parse::<i64>() {
        return serde_json::Value::Number(parsed.into());
    }

    if let Ok(parsed) = trimmed.parse::<f64>()
        && let Some(number) = serde_json::Number::from_f64(parsed)
    {
        return serde_json::Value::Number(number);
    }

    serde_json::Value::String(value)
}

fn normalize_tagged_parameter_value(raw: &str) -> String {
    let mut value = raw.replace("\r\n", "\n");
    if value.starts_with('\n') {
        value.remove(0);
    }
    if value.ends_with('\n') {
        value.pop();
    }
    value
}

#[cfg(test)]
mod tests {
    use super::{NormalisedChunk, StreamTextNormaliser};
    use serde_json::json;

    #[test]
    fn normalise_splits_text_and_tagged_tool_call() {
        let mut normaliser = StreamTextNormaliser::new();
        let chunks = normaliser.normalise(
            "Let me inspect that.\n<function=read_file>\n<parameter=path>file.txt</parameter>\n<parameter=offset>3</parameter>\n</function>\nAfter the read.",
        );

        assert_eq!(
            chunks,
            vec![
                NormalisedChunk::Text("Let me inspect that.\n".to_string()),
                NormalisedChunk::TaggedToolCall {
                    name: "read_file".to_string(),
                    input: json!({"path": "file.txt", "offset": 3}),
                },
                NormalisedChunk::Text("\nAfter the read.".to_string()),
            ]
        );
    }

    #[test]
    fn normalise_buffers_incomplete_function_until_closed() {
        let mut normaliser = StreamTextNormaliser::new();
        let first = normaliser.normalise("preface\n<function=read_file>\n<parameter=path>");
        assert_eq!(first, vec![NormalisedChunk::Text("preface\n".to_string())]);

        let second = normaliser.normalise("file.txt</parameter>\n</function>");
        assert_eq!(
            second,
            vec![NormalisedChunk::TaggedToolCall {
                name: "read_file".to_string(),
                input: json!({"path": "file.txt"}),
            }]
        );
    }

    #[test]
    fn normalise_qwen3_coder_tool_call_with_outer_wrapper() {
        let mut normaliser = StreamTextNormaliser::new();
        let chunks = normaliser.normalise(
            "<tool_call>\n<function=read_file>\n<parameter=path>src/lib.rs</parameter>\n</function>\n</tool_call>",
        );
        assert_eq!(
            chunks,
            vec![
                NormalisedChunk::Text("\n".to_string()),
                NormalisedChunk::TaggedToolCall {
                    name: "read_file".to_string(),
                    input: json!({"path": "src/lib.rs"}),
                },
                NormalisedChunk::Text("\n".to_string()),
            ]
        );
    }

    #[test]
    fn normalise_hermes_json_tool_call() {
        let mut normaliser = StreamTextNormaliser::new();
        let chunks = normaliser.normalise(
            "Let me read it.\n<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"Cargo.toml\"}}\n</tool_call>",
        );
        assert_eq!(
            chunks,
            vec![
                NormalisedChunk::Text("Let me read it.\n".to_string()),
                NormalisedChunk::TaggedToolCall {
                    name: "read_file".to_string(),
                    input: json!({"path": "Cargo.toml"}),
                },
            ]
        );
    }

    #[test]
    fn normalise_hermes_json_tool_call_string_arguments() {
        let mut normaliser = StreamTextNormaliser::new();
        let chunks = normaliser.normalise(
            "<tool_call>{\"name\": \"search_files\", \"arguments\": \"{\\\"pattern\\\": \\\"*.rs\\\"}\"}</tool_call>",
        );
        assert_eq!(
            chunks,
            vec![NormalisedChunk::TaggedToolCall {
                name: "search_files".to_string(),
                input: json!({"pattern": "*.rs"}),
            }]
        );
    }

    #[test]
    fn normalise_buffers_incomplete_hermes_tool_call() {
        let mut normaliser = StreamTextNormaliser::new();
        let first = normaliser
            .normalise("<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\":");
        assert_eq!(first, vec![]);

        let second = normaliser.normalise(" \"README.md\"}}\n</tool_call>");
        assert_eq!(
            second,
            vec![NormalisedChunk::TaggedToolCall {
                name: "read_file".to_string(),
                input: json!({"path": "README.md"}),
            }]
        );
    }

    #[test]
    fn normalise_function_with_json_body_fallback() {
        let mut normaliser = StreamTextNormaliser::new();
        let chunks =
            normaliser.normalise("<function=read_file>\n{\"path\": \"src/main.rs\"}\n</function>");
        assert_eq!(
            chunks,
            vec![NormalisedChunk::TaggedToolCall {
                name: "read_file".to_string(),
                input: json!({"path": "src/main.rs"}),
            }]
        );
    }
}
