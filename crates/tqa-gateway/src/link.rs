//! Bounded runtime registration and typed runtime input handling.

mod endpoint;
mod history;
mod inbox;
mod mcp;
mod session;
pub(crate) use history::history;
pub(crate) use session::{fallback_snapshots, first_response_secs, route_snapshot};
mod liveness;
mod protocol;
mod registry;
mod renewals;
mod socket;
mod workspace;
pub(crate) use endpoint::upgrade;
pub(crate) use protocol::{
    FrameSink, Received, StoreBounds, plain_journal, receive, receive_batch,
};
pub use registry::RuntimeHeartbeat;
pub(crate) use registry::{LinkRegistry, Registration};
pub(crate) use workspace::{BLOB_BATCH_BYTES, BLOB_BATCH_HASHES};
#[cfg(test)]
mod batch_append_tests;
#[cfg(test)]
mod batch_tests;
#[cfg(test)]
mod outage_tests;
#[cfg(test)]
mod store_bounds_tests;
#[cfg(test)]
mod tests;
