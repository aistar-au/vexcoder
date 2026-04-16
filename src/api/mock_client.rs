use crate::api::client::MockStreamProducer;
use crate::api::stream::StreamParser;
use crate::runtime::backend::EventStream;
use crate::types::ApiMessage;
use anyhow::Result;
use futures::stream;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct MockApiClient {
    responses: Arc<Mutex<Vec<Vec<String>>>>,
}

impl MockApiClient {
    pub fn new(responses: Vec<Vec<String>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
        }
    }
}

#[cfg(test)]
impl MockStreamProducer for MockApiClient {
    fn create_mock_stream(&self, _messages: &[ApiMessage]) -> Result<EventStream> {
        let mut responses_guard = self.responses.lock().unwrap();
        if responses_guard.is_empty() {
            return Err(anyhow::anyhow!(
                "MockApiClient: No more responses configured"
            ));
        }
        let current_sse_chunks = responses_guard.remove(0);
        let mut parser = StreamParser::new();
        let mut events = Vec::new();

        for s in current_sse_chunks {
            let framed = if s.ends_with("\n\n") {
                s
            } else {
                format!("{s}\n\n")
            };
            events.extend(parser.process(framed.as_bytes())?);
        }

        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}
