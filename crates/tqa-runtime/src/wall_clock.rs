//! Event timestamps are independent of the injected monotonic deadlines.
use crate::RuntimeError;

/// Supplies UTC RFC 3339 timestamps without exposing wall-clock arithmetic to the core.
pub trait WallClock: Send + Sync {
    /// Returns a valid UTC timestamp for a journaled event.
    ///
    /// # Errors
    /// Returns a content-free I/O error if the timestamp cannot be represented.
    fn timestamp(&self) -> Result<String, RuntimeError>;
}
/// The process wall clock, formatted as UTC RFC 3339.
pub struct SystemWallClock;
impl WallClock for SystemWallClock {
    fn timestamp(&self) -> Result<String, RuntimeError> {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| RuntimeError::io("wall clock format"))
    }
}
