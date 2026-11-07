//! Typed failures for the private runtime write-ahead log.

/// A WAL operation failed before the caller may rely on its result.
#[derive(Debug, thiserror::Error)]
pub enum WalError {
    /// Another writer holds the stable state-directory lock.
    #[error("write-ahead log is locked")]
    Locked,
    /// The 64 MiB file capacity would be exceeded.
    #[error("write-ahead log is full")]
    Full,
    /// A sequence is missing, duplicated, reordered or exhausted.
    #[error(transparent)]
    Sequence(#[from] tqa_types::SeqError),
    /// A frame is invalid, unsequenced or over the wire limit.
    #[error("frame cannot be stored in the write-ahead log")]
    InvalidFrame,
    /// A complete record is malformed or its sequence does not match its frame.
    #[error("corrupt write-ahead log record")]
    Corrupt,
    /// A failed append could not be rolled back; close and recover before further writes.
    #[error("write-ahead log requires recovery")]
    NeedsRecovery,
    /// Storage or synchronization failed.
    #[error("write-ahead log I/O failed")]
    Io(#[from] std::io::Error),
}
