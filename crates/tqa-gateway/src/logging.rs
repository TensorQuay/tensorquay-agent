//! The gateway's log filter: `TQA_LOG`, in `RUST_LOG` syntax, or the built-in default. Read once at start; an invalid
//! value stops the start instead of silently logging at another level (ENGINEERING-GUARDRAILS §1.4, §1.13).
use crate::GatewayError;
use tracing_subscriber::EnvFilter;

/// The environment variable that overrides the filter, for diagnosis (for example `info,tqa_gateway::relay=debug`).
pub const VARIABLE: &str = "TQA_LOG";
/// Info for the gateway; its chattier dependencies only warn.
pub const DEFAULT_FILTER: &str = "info,bollard=warn,hyper=warn,sqlx=warn";
/// The longest filter accepted: a few dozen directives, far more than diagnosis needs.
pub const MAX_FILTER_BYTES: usize = 1024;

/// The filter for `value` (the variable's value, if set): unset or blank gives [`DEFAULT_FILTER`].
///
/// # Errors
/// `Configuration` for a filter that does not parse or is longer than [`MAX_FILTER_BYTES`].
pub fn filter(value: Option<&str>) -> Result<EnvFilter, GatewayError> {
    let value = value.map(str::trim).filter(|value| !value.is_empty());
    let Some(value) = value else {
        return Ok(EnvFilter::new(DEFAULT_FILTER));
    };
    if value.len() > MAX_FILTER_BYTES {
        return Err(GatewayError::Configuration(
            "TQA_LOG is longer than 1024 bytes",
        ));
    }
    EnvFilter::try_new(value)
        .map_err(|_| GatewayError::Configuration("TQA_LOG is not a valid log filter"))
}

#[cfg(test)]
#[path = "logging_tests.rs"]
mod tests;
