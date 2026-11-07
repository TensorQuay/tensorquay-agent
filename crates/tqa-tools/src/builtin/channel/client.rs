//! What the channel tools need from the gateway (the runtime's channel routes, ADR-0014 point 6). The runtime supplies
//! the implementation, bound to the gateway's origin and the run's token; the tools never build a URL.
use super::ChannelPage;
use async_trait::async_trait;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tqa_types::{ChannelFilter, ChannelMessage, ChannelPost};

/// What one read or wait asks for.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReadRequest {
    /// Messages after this seq.
    pub since: u64,
    /// Which messages.
    pub filter: ChannelFilter,
    /// At most this many.
    pub limit: u32,
    /// Wait this long for a first match; `None` reads at once.
    pub wait: Option<Duration>,
}

/// One channel the run's conversation belongs to.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize)]
pub struct ChannelSummary {
    /// The channel's id.
    pub id: String,
    /// Its name, written by a person.
    pub name: String,
    /// Its purpose, written by a person.
    #[serde(default)]
    pub purpose: String,
    /// The conversation's role: `lead`, `member` or `observer`.
    pub role: String,
    /// The decision rule: `human` or `majority`.
    pub rule: String,
    /// `open` or `ended`.
    pub state: String,
}

/// A claim the gateway recorded; `existing` when the caller already held it (a safe retry).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Claimed {
    /// The claim message.
    pub message: ChannelMessage,
    /// Whether the claim was already the caller's.
    pub existing: bool,
}

/// A refused or failed call: the gateway's stable code and a content-free sentence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChannelFailure {
    /// A stable code such as `not_member` or `already_claimed`.
    pub code: String,
    /// What it means, for the model.
    pub message: String,
    /// The refusal's `details`, when the gateway sent any: an agent team's `team_ended` carries the lead's unread
    /// messages there (ADR-0034 point 10).
    pub details: Option<serde_json::Value>,
}

impl ChannelFailure {
    /// A failure with `code` and `message`.
    #[must_use]
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
            details: None,
        }
    }

    /// The same failure with the refusal's `details`.
    #[must_use]
    pub fn with_details(mut self, details: Option<serde_json::Value>) -> Self {
        self.details = details;
        self
    }
}

/// The runtime channel routes. Every call ends promptly when `cancel` fires.
#[async_trait]
pub trait ChannelClient: Send + Sync {
    /// The channels the run's conversation belongs to.
    async fn channels(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Vec<ChannelSummary>, ChannelFailure>;
    /// The messages `request` selects; with a wait, a long poll that returns as soon as one exists, or empty when it
    /// runs out.
    async fn read(
        &self,
        channel: &str,
        request: &ReadRequest,
        cancel: &CancellationToken,
    ) -> Result<ChannelPage, ChannelFailure>;
    /// Posts; `key` (when not empty) makes a retry return the first answer.
    async fn post(
        &self,
        channel: &str,
        post: &ChannelPost,
        key: &str,
        cancel: &CancellationToken,
    ) -> Result<ChannelMessage, ChannelFailure>;
    /// Claims a task; `key` as for [`ChannelClient::post`].
    async fn claim(
        &self,
        channel: &str,
        task_seq: u64,
        key: &str,
        cancel: &CancellationToken,
    ) -> Result<Claimed, ChannelFailure>;
}
