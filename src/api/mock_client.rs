use crate::api::client::MockStreamProducer;
use crate::api::stream::StreamParser;
use crate::runtime::backend::EventStream;
use crate::types::ApiMessage;
use anyhow::Result;
use futures::stream;
use std::collections::VecDeque;
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
        let chunks = responses_guard.remove(0);

        
        let stream = stream::try_unfold(
            (
                chunks.into_iter(),
                StreamParser::new(),
                VecDeque::new(),
                false,
            ),
            |(mut chunks, mut parser, mut pending, mut finished)| async move {
                loop {
                    if let Some(event) = pending.pop_front() {
                        return Ok(Some((event, (chunks, parser, pending, finished))));
                    }
                    match chunks.next() {
                        None => {
                            if finished {
                                return Ok(None);
                            }
                            finished = true;
                            pending.extend(parser.finish());
                            continue;
                        }
                        Some(chunk) => {
                            let framed = if chunk.ends_with("\n\n") {
                                chunk
                            } else {
                                format!("{chunk}\n\n")
                            };
                            let events = parser.process(framed.as_bytes())?;
                            pending.extend(events);
                        }
                    }
                }
            },
        );

        Ok(Box::pin(stream))
    }
}
