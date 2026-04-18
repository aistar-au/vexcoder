use super::provider::{ProviderApiStreamError, ProviderStreamEvent};
use super::{MAX_SSE_BUFFER_BYTES, StreamParser, StreamProtocolMode};
use crate::runtime::RuntimeEnvelope;
use anyhow::Result;

impl StreamParser {
    pub fn process_sse_event(
        &mut self,
        event_type: &str,
        data: &str,
    ) -> Result<Vec<RuntimeEnvelope>> {
        self.parse_event_payload((!event_type.is_empty()).then_some(event_type), data)
    }

    pub fn process(&mut self, chunk: &[u8]) -> Result<Vec<RuntimeEnvelope>> {
        if self.overflowed {
            return Ok(vec![self.buffer_overflow_event()]);
        }

        if self.buffer.len().saturating_add(chunk.len()) > MAX_SSE_BUFFER_BYTES {
            self.overflowed = true;
            return Ok(vec![self.buffer_overflow_event()]);
        }
        self.buffer.extend_from_slice(chunk);
        self.strip_utf8_bom_once();

        let mut events = Vec::new();

        while let Some((pos, delim_len)) = self.find_delimiter() {
            let end = pos + delim_len;
            let frame_bytes = self.buffer[..pos].to_vec();
            self.buffer.drain(..end);
            events.extend(self.parse_frame_bytes(frame_bytes)?);
        }

        Ok(events)
    }

    fn parse_frame_bytes(&mut self, frame_bytes: Vec<u8>) -> Result<Vec<RuntimeEnvelope>> {
        let frame_text = String::from_utf8(frame_bytes)?;
        let normalised_frame = normalise_sse_line_endings(&frame_text);
        let mut event_type = None;
        let mut data_lines = Vec::new();

        for line in normalised_frame.split('\n') {
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("event:") {
                event_type = Some(strip_single_leading_space(rest).to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                data_lines.push(strip_single_leading_space(rest).to_string());
            } else if let Some(rest) = line.strip_prefix("id:") {
                let val = strip_single_leading_space(rest);
                if !val.contains('\0') {
                    self.last_event_id = Some(val.to_string());
                }
            } else if line == "id" {
                self.last_event_id = Some(String::new());
            } else if let Some(rest) = line.strip_prefix("retry:")
                && let Ok(ms) = strip_single_leading_space(rest).parse::<u64>()
            {
                self.reconnect_delay_ms = Some(ms);
            }
        }

        if data_lines.is_empty() {
            if frame_without_data_should_error(&normalised_frame) {
                return Ok(vec![self.provider_error_envelope(
                    "sse_parse_error",
                    "received a non-SSE payload without a data field; raw JSON chunk streams are unsupported"
                        .to_string(),
                )]);
            }
            return Ok(Vec::new());
        }

        let json_data = data_lines.join("\n");

        self.parse_event_payload(event_type.as_deref(), &json_data)
    }

    fn parse_event_payload(
        &mut self,
        event_type: Option<&str>,
        json_data: &str,
    ) -> Result<Vec<RuntimeEnvelope>> {
        if json_data.is_empty() {
            return Ok(Vec::new());
        }

        if let Ok(envelope) = serde_json::from_str::<RuntimeEnvelope>(json_data) {
            self.protocol_mode = StreamProtocolMode::RuntimeEnvelope;
            return Ok(vec![envelope]);
        }

        let events = self.parse_legacy_event_payload(event_type, json_data);
        Ok(self.normalize_provider_events(events))
    }

    fn parse_legacy_event_payload(
        &mut self,
        event_type: Option<&str>,
        json_data: &str,
    ) -> Vec<ProviderStreamEvent> {
        if event_type == Some("ping") {
            return vec![ProviderStreamEvent::Ping];
        }

        match serde_json::from_str::<ProviderStreamEvent>(json_data) {
            Ok(evt) => vec![evt],
            Err(messages_v1_error) => {
                if let Some(chat_compat_events) = self.parse_chat_compat_chunk(json_data) {
                    chat_compat_events
                } else {
                    super::super::logging::emit_sse_parse_error(
                        event_type,
                        json_data,
                        &messages_v1_error,
                    );
                    vec![ProviderStreamEvent::Error {
                        error: ProviderApiStreamError {
                            error_type: "sse_parse_error".to_string(),
                            message: messages_v1_error.to_string(),
                        },
                    }]
                }
            }
        }
    }

    fn find_delimiter(&self) -> Option<(usize, usize)> {
        let mut index = 0;
        while index < self.buffer.len() {
            let Some(first_len) = line_terminator_len(&self.buffer, index) else {
                index += 1;
                continue;
            };
            let next = index + first_len;
            if let Some(second_len) = line_terminator_len(&self.buffer, next) {
                return Some((index, first_len + second_len));
            }
            index = next;
        }
        None
    }

    fn strip_utf8_bom_once(&mut self) {
        if self.bom_checked {
            return;
        }

        const UTF8_BOM: &[u8; 3] = b"\xEF\xBB\xBF";
        if self.buffer.starts_with(UTF8_BOM) {
            self.buffer.drain(..UTF8_BOM.len());
            self.bom_checked = true;
            return;
        }

        if self.buffer.is_empty() {
            return;
        }

        let prefix_len = self.buffer.len().min(UTF8_BOM.len());
        if prefix_len == UTF8_BOM.len() || self.buffer[..prefix_len] != UTF8_BOM[..prefix_len] {
            self.bom_checked = true;
        }
    }

    fn buffer_overflow_event(&mut self) -> RuntimeEnvelope {
        self.provider_error_envelope(
            "sse_buffer_overflow",
            format!(
                "SSE intra-frame buffer exceeded {MAX_SSE_BUFFER_BYTES} bytes without a \
                 frame delimiter; the upstream stream may be malformed"
            ),
        )
    }

    pub fn last_event_id(&self) -> Option<&str> {
        self.last_event_id.as_deref()
    }

    pub fn reconnect_delay_ms(&self) -> Option<u64> {
        self.reconnect_delay_ms
    }
}

fn strip_single_leading_space(value: &str) -> &str {
    value.strip_prefix(' ').unwrap_or(value)
}

fn normalise_sse_line_endings(frame: &str) -> String {
    frame.replace("\r\n", "\n").replace('\r', "\n")
}

fn frame_without_data_should_error(frame: &str) -> bool {
    frame
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(':'))
        .any(looks_like_raw_json_payload)
}

fn looks_like_raw_json_payload(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && (trimmed == "[DONE]"
            || ((trimmed.starts_with('{') || trimmed.starts_with('['))
                && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()))
}

fn line_terminator_len(buffer: &[u8], index: usize) -> Option<usize> {
    match buffer.get(index) {
        Some(b'\n') => Some(1),
        Some(b'\r') => {
            if buffer.get(index + 1) == Some(&b'\n') {
                Some(2)
            } else {
                Some(1)
            }
        }
        _ => None,
    }
}
