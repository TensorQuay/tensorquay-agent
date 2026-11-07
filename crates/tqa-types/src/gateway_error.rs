//! The gateway's own refusals on its relays (the model relay, the engine relay and the MCP proxy): a response the
//! gateway answers itself carries [`GATEWAY_ERROR_HEADER`], naming its stable error code (the body's `error.code`).
//! The relays strip the header from anything forwarded from upstream, so a provider or connector can't send it.

/// The response header naming the gateway's own refusal.
pub const GATEWAY_ERROR_HEADER: &str = "x-tqa-gateway-error";

/// The longest code the header may carry.
const MAX_CODE: usize = 64;

/// The code a header value names: a bare lowercase token (`[a-z]` first, then `[a-z0-9_]`, at most 64 bytes); any
/// other value names none, and the response is read as if the header were absent.
#[must_use]
pub fn gateway_error_code(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    let first = *bytes.first()?;
    (bytes.len() <= MAX_CODE
        && first.is_ascii_lowercase()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_'))
    .then_some(value)
}
