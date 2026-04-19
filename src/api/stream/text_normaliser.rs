#[derive(Default)]
pub struct StreamTextNormaliser {
    pub(super) pending: String,
}

pub enum NormalisedChunk {
    Text(String),
}

impl StreamTextNormaliser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn normalise(&mut self, text: &str) -> Vec<NormalisedChunk> {
        if text.is_empty() {
            Vec::new()
        } else {
            vec![NormalisedChunk::Text(text.to_string())]
        }
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }

    pub fn flush(&mut self) -> Vec<NormalisedChunk> {
        let pending = std::mem::take(&mut self.pending);
        if pending.is_empty() {
            Vec::new()
        } else {
            vec![NormalisedChunk::Text(pending)]
        }
    }
}
