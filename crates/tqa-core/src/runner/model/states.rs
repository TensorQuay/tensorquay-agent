//! Opaque reasoning states (ADR-0027): journaled within their bounds, in stream order before the
//! response's calls, and replayed only to the route and model that produced them.
use super::{Execution, Stop};
use tqa_context::ReasoningStateRecord;
use tqa_types::{
    Event, MAX_FRAME_BYTES, MAX_REASONING_BYTES_PER_STEP, MAX_REASONING_ITEMS_PER_STEP,
    ReasoningItem, RouteSnapshot,
};

/// Room left in a frame for the event envelope around an item.
const ENVELOPE_BYTES: usize = 4096;

impl Execution<'_> {
    /// Records each state of a completed response. A state over a bound keeps its record, with its
    /// size but without the item, so a later request knows the state was lost.
    pub(super) async fn record_states(&mut self, states: &[String]) -> Result<(), Stop> {
        let route = self.route().clone();
        let (mut kept, mut bytes) = (0_usize, 0_usize);
        for state in states {
            let fits = kept < MAX_REASONING_ITEMS_PER_STEP
                && bytes.saturating_add(state.len()) <= MAX_REASONING_BYTES_PER_STEP
                && escaped_len(state).saturating_add(ENVELOPE_BYTES) <= MAX_FRAME_BYTES;
            let item = fits
                .then(|| ReasoningItem::new(state.clone()).ok())
                .flatten();
            if item.is_some() {
                kept += 1;
                bytes = bytes.saturating_add(state.len());
            }
            self.emit(Event::ReasoningState {
                protocol: route.protocol,
                route: route.name.clone(),
                model: route.model.clone(),
                bytes: u32::try_from(state.len()).unwrap_or(u32::MAX),
                item,
            })
            .await?;
        }
        Ok(())
    }
}

/// The item's length once escaped as a JSON string inside the event frame.
fn escaped_len(item: &str) -> usize {
    serde_json::to_string(item).map_or(usize::MAX, |text| text.len())
}

/// The kept items of an assistant message that may be replayed to `route`: recorded on the same
/// route, protocol and model.
pub(super) fn replayable<'a>(
    states: &'a [ReasoningStateRecord],
    route: &'a RouteSnapshot,
) -> impl Iterator<Item = &'a str> {
    states
        .iter()
        .filter(move |state| replays(state, route))
        .filter_map(|state| state.item.as_deref())
}

/// Whether a recorded state goes back to this route: only to the same route, protocol and model (ADR-0027).
fn replays(state: &ReasoningStateRecord, route: &RouteSnapshot) -> bool {
    state.route == route.name && state.protocol == route.protocol && state.model == route.model
}

/// The history as a request sends it, and the tokens its latest assistant message's states take. Each message keeps
/// only the states replayed to this route, so the window counts exactly what goes (CORE-U-074). While the request would
/// need more than `limit`, the oldest replayed states go first, message by message: a provider accepts a history
/// without them and loses only that turn's thinking. The latest assistant message's states always stay, since its tool
/// calls need them (CORE-U-044).
/// `needed` and `scale` count at the route's density (CORE-U-084): a dropped message's states come off the total at
/// that density too, so no more go than the limit needs.
pub(super) fn sendable(
    mut history: Vec<tqa_context::Message>,
    route: &RouteSnapshot,
    limit: u64,
    needed: impl Fn(&[tqa_context::Message]) -> u64,
    scale: impl Fn(u64) -> u64,
) -> (Vec<tqa_context::Message>, u64) {
    for message in &mut history {
        message
            .reasoning_states
            .retain(|state| replays(state, route) && state.item.is_some());
    }
    let latest = history
        .iter()
        .rposition(|message| message.role == tqa_context::Role::Assistant);
    let mut total = needed(&history);
    for (index, message) in history.iter_mut().enumerate() {
        if total <= limit {
            break;
        }
        if Some(index) == latest || message.reasoning_states.is_empty() {
            continue;
        }
        total = total.saturating_sub(scale(states_tokens(message)));
        message.reasoning_states.clear();
    }
    let kept = latest.map_or(0, |index| states_tokens(&history[index]));
    (history, kept)
}

/// The tokens a message's states take in a request.
fn states_tokens(message: &tqa_context::Message) -> u64 {
    let without = tqa_context::Message {
        reasoning_states: Vec::new(),
        ..message.clone()
    };
    u64::from(tqa_context::message_tokens(message).tokens)
        .saturating_sub(u64::from(tqa_context::message_tokens(&without).tokens))
}

#[cfg(test)]
#[path = "states_tests.rs"]
mod tests;
