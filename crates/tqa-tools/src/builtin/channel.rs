//! The channel tools (ADR-0014 point 6): `channel_post`, `channel_read`, `channel_wait` and `task_claim`. They reach the
//! gateway's runtime channel routes through a [`ChannelClient`] the runtime supplies; what they return is marked as
//! channel content, never instructions (point 10, SEC-035).
mod client;
pub(crate) mod format;
pub(crate) mod post;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tool_tests;
mod tools;
pub use tools::idempotency_key;

pub use client::{ChannelClient, ChannelFailure, ChannelSummary, Claimed, ReadRequest};
pub use tools::{ChannelPostTool, ChannelRead, TaskClaim, tools, tools_for_run};
use tqa_types::ChannelMessage;

/// One page of a channel's log, as the gateway returns it.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChannelPage {
    /// The selected messages after `since`, in seq order.
    pub messages: Vec<PageMessage>,
    /// The cursor to continue from.
    pub next_since: u64,
    /// Whether more selected messages follow `next_since`.
    pub more: bool,
}

/// A message of a page: one this runtime reads, or one a later gateway sent that it doesn't recognise (a new type,
/// rule, outcome or field), kept as the gateway's JSON so a page never fails for it (ADR-0034 point 12a). The types
/// stay strict for what the runtime posts.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum PageMessage {
    /// A message this runtime reads.
    Known(ChannelMessage),
    /// Any other message, as the gateway sent it.
    Unrecognised(serde_json::Value),
}

impl PageMessage {
    /// The message's seq, when the gateway gave one.
    #[must_use]
    pub fn seq(&self) -> Option<u64> {
        match self {
            Self::Known(message) => Some(message.seq),
            Self::Unrecognised(value) => value["seq"].as_u64(),
        }
    }
}

impl From<ChannelMessage> for PageMessage {
    fn from(message: ChannelMessage) -> Self {
        Self::Known(message)
    }
}
