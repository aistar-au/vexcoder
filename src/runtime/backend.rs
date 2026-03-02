use anyhow::Result;
use bytes::Bytes;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::types::ApiMessage;

pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelBackendKind {
    LocalRuntime,
    ApiServer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelProtocol {
    MessagesV1,
    ChatCompat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCallMode {
    Structured,
    TaggedFallback,
}

impl ModelProtocol {
    pub fn request_headers(self) -> Vec<(&'static str, &'static str)> {
        match self {
            ModelProtocol::MessagesV1 => vec![("anthropic-version", "2023-06-01")],
            ModelProtocol::ChatCompat => vec![],
        }
    }
}

pub trait ModelBackend: Send + Sync {
    fn backend_kind(&self) -> ModelBackendKind;
    fn protocol(&self) -> ModelProtocol;
    fn supports_structured_tools(&self) -> bool;
    fn is_local(&self) -> bool;
    fn create_stream(
        &self,
        messages: &[ApiMessage],
    ) -> impl std::future::Future<Output = Result<ByteStream>> + Send;
}
