#[derive(Default)]
pub struct StreamTextNormaliser;

pub enum NormalisedChunk {
    Text(String),
}

impl StreamTextNormaliser {
    pub fn new() -> Self {
        Self
    }

    pub fn normalise(&mut self, text: &str) -> Vec<NormalisedChunk> {
        if text.is_empty() {
            Vec::new()
        } else {
            vec![NormalisedChunk::Text(text.to_string())]
        }
    }
}
