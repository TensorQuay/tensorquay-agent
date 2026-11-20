//! `[channels]`: how much woken runs may spend in a channel with no budget of its own (ADR-0014's amendment of 20 Nov
//! 2026, A5).
use serde::Deserialize;

/// The tokens a channel's woken runs may spend when the channel has no budget of its own, by default.
const DEFAULT_WAKE_BUDGET: u64 = 300_000;

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ChannelsInput {
    wake_budget: Option<u64>,
}

/// The channels' settings as the file sets them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Channels {
    /// Tokens for a channel without its own budget; 0 turns waking off for such channels.
    pub(crate) wake_budget: u64,
}

impl Default for Channels {
    fn default() -> Self {
        Self {
            wake_budget: DEFAULT_WAKE_BUDGET,
        }
    }
}

/// The section with its defaults.
pub(super) fn channels(input: &ChannelsInput) -> Channels {
    Channels {
        wake_budget: input.wake_budget.unwrap_or(DEFAULT_WAKE_BUDGET),
    }
}
