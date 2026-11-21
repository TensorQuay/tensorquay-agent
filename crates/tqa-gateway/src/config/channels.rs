//! `[channels]`: how much woken runs may spend a day in a channel with no budget of its own (ADR-0014's amendment of 20
//! Nov 2026, A5; a daily allowance, decided on 21 Nov 2026, C-3).
use crate::GatewayError;
use serde::Deserialize;
use std::time::Duration;

/// The tokens a channel's woken runs may spend a day when the channel has no budget of its own, by default.
const DEFAULT_WAKE_BUDGET: u64 = 2_000_000;
/// How long a channel's budget window lasts from its first charge: a day.
const DEFAULT_BUDGET_WINDOW_SECONDS: u32 = 86_400;
/// The most a channel's own budget may be set to, by default (decided on 20 Nov 2026, C-1).
const DEFAULT_MAX_WAKE_BUDGET: u64 = 10_000_000;

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ChannelsInput {
    wake_budget: Option<u64>,
    max_wake_budget: Option<u64>,
    budget_window_seconds: Option<u32>,
}

/// The channels' settings as the file sets them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Channels {
    /// Tokens a day for a channel without its own budget; 0 turns waking off for such channels.
    pub(crate) wake_budget: u64,
    /// The most a channel's lead or owner may set its own budget to: the administrator's ceiling.
    pub(crate) max_wake_budget: u64,
    /// How long a budget window lasts from its first charge (a day; tests shorten it); spent is 0 again after it.
    pub(crate) budget_window: Duration,
}

impl Default for Channels {
    fn default() -> Self {
        Self {
            wake_budget: DEFAULT_WAKE_BUDGET,
            max_wake_budget: DEFAULT_MAX_WAKE_BUDGET,
            budget_window: Duration::from_secs(u64::from(DEFAULT_BUDGET_WINDOW_SECONDS)),
        }
    }
}

/// The section with its defaults.
///
/// # Errors
/// A window shorter than a second or longer than a week.
pub(super) fn channels(input: &ChannelsInput) -> Result<Channels, GatewayError> {
    let window = input
        .budget_window_seconds
        .unwrap_or(DEFAULT_BUDGET_WINDOW_SECONDS);
    if !(1..=604_800).contains(&window) {
        return Err(GatewayError::Configuration(
            "channels.budget_window_seconds must be 1 through 604800",
        ));
    }
    Ok(Channels {
        wake_budget: input.wake_budget.unwrap_or(DEFAULT_WAKE_BUDGET),
        max_wake_budget: input.max_wake_budget.unwrap_or(DEFAULT_MAX_WAKE_BUDGET),
        budget_window: Duration::from_secs(u64::from(window)),
    })
}

#[cfg(test)]
#[path = "channels_tests.rs"]
mod tests;
