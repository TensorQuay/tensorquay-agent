//! SSE framing and a checked replay cursor with a bounded live subscription.

mod endpoint;
mod options;
mod session;
mod snapshot;
pub(crate) use endpoint::events;
pub(crate) use options::EventQuery;

use crate::{GatewayError, wire};
use axum::response::sse::Event;
use tqa_types::ServerFrame;

pub(crate) fn encode(frame: &ServerFrame) -> Result<Event, GatewayError> {
    let data = serde_json::to_string(frame).map_err(GatewayError::from)?;
    let event = Event::default().data(data);
    Ok(
        wire::sequence(frame)
            .map_or_else(|| event.clone(), |seq| event.clone().id(seq.to_string())),
    )
}

#[cfg(test)]
mod tests;
