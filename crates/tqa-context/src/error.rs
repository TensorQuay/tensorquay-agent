//! Typed context assembly and spill failures.

/// A complete request or bounded tool preview could not be prepared.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    /// The full request does not fit; no history was silently removed.
    #[error("context budget exceeded: needed {needed}, available {available}")]
    OverBudget {
        /// Estimated input tokens including message overhead.
        needed: u64,
        /// Tokens available after the output reservation.
        available: u32,
    },
    /// A preview must permit at least 1024 bytes for its marker, head and tail.
    #[error("tool result byte limit must be at least 1024")]
    InvalidLimit,
    /// A spill directory must end with a validated conversation spill suffix.
    #[error("invalid conversation spill directory")]
    InvalidSpillPath,
    /// A spill could not be written; the caller must not claim it was saved.
    #[error("context storage I/O failed")]
    Io(#[from] std::io::Error),
}
