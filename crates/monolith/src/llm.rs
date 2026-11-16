//! The model client: one chat completion over plain HTTP to a loopback address.
//! STUB written by the leader: signatures are pinned, bodies belong to worker A2.

use std::time::Duration;

use aci::envelope::Code;
use aci::json::Json;

use crate::types::{Error, Result};

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The harness's standing instructions.
    System,
    /// The task and the observations.
    User,
    /// The model's earlier turns.
    Assistant,
}

/// One chat message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Who wrote it.
    pub role: Role,
    /// Its text.
    pub content: String,
}

/// One chat completion request.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest<'a> {
    /// Base URL, loopback only, for example `http://127.0.0.1:8080`.
    pub url: &'a str,
    /// Model name.
    pub model: &'a str,
    /// The conversation so far.
    pub messages: &'a [Message],
    /// Most tokens to write.
    pub max_tokens: u32,
    /// Sampling temperature.
    pub temperature: f64,
    /// Sampling seed.
    pub seed: i64,
    /// When set, the reply must be JSON matching this schema (sent as `response_format`).
    pub json_schema: Option<&'a Json>,
    /// Let the model think first (`chat_template_kwargs.enable_thinking`).
    pub thinking: bool,
    /// Give up after this long.
    pub timeout: Duration,
}

/// The model's reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// `choices[0].message.content`.
    pub content: String,
    /// `usage.prompt_tokens` (0 if absent).
    pub prompt_tokens: u64,
    /// `usage.completion_tokens` (0 if absent).
    pub completion_tokens: u64,
    /// Wall time in milliseconds.
    pub ms: u64,
}

/// Sends the request to `POST {url}/v1/chat/completions` and returns the reply.
/// `denied` for a non-loopback host, `io` for connection trouble, `invalid` for a bad response.
pub fn chat(_req: &ChatRequest<'_>) -> Result<Reply> {
    Err(Error::new(Code::Unsupported, "llm: not built yet"))
}
