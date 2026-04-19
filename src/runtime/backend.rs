use anyhow::Result;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::runtime::RuntimeEnvelope;
use crate::types::ApiMessage;

pub type EventStream = Pin<Box<dyn Stream<Item = Result<RuntimeEnvelope>> + Send>>;

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
}

/// Controls which tools are exposed to the model for a session.
///
/// - `Full`: all registered tools including mutating and shell tools.
/// - `Plan`: read-only tools only (search, read, list, git read ops, codebase_search, MCP).
/// - `Chat`: no tools — plain conversation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ToolPolicy {
    #[default]
    Full,
    Plan,
    Chat,
}

pub trait ModelBackend: Send + Sync {
    fn backend_kind(&self) -> ModelBackendKind;
    fn protocol(&self) -> ModelProtocol;
    fn supports_structured_tools(&self) -> bool;
    fn is_local(&self) -> bool;
    fn create_stream(
        &self,
        messages: &[ApiMessage],
    ) -> impl std::future::Future<Output = Result<EventStream>> + Send;
}
