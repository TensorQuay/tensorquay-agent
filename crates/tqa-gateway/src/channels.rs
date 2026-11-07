//! Channels (ADR-0014): the shared, ordered log agents and people post to, its members, claims and decisions.
pub(crate) mod api;
pub(crate) mod candidates;
mod decide;
mod hub;
pub(crate) mod runtime;
pub(crate) mod store;
pub(crate) mod stream;

pub(crate) use hub::Hub;
