//! Explicit wire-version negotiation shared by both transport bindings.

use crate::{ErrorCode, PROTOCOL_VERSION};

/// A peer requested a version outside the supported protocol set.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("unsupported protocol {requested}; supported versions: {supported:?}")]
pub struct UnsupportedProtocol {
    /// The version requested by the peer.
    pub requested: u16,
    /// The versions this implementation can speak.
    pub supported: Vec<u16>,
}

impl UnsupportedProtocol {
    /// Returns the stable negotiation error code.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        ErrorCode::UnsupportedProtocol
    }
}

/// Accepts protocol version one for either wire binding.
///
/// # Errors
/// Returns the requested version and supported set for every other version.
pub fn check_protocol(requested: u16) -> Result<(), UnsupportedProtocol> {
    if requested == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(UnsupportedProtocol {
            requested,
            supported: vec![PROTOCOL_VERSION],
        })
    }
}
