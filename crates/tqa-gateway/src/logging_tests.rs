//! GW-U-034: the gateway's log filter comes from `TQA_LOG`; unset or empty keeps the built-in default (info).
use super::{DEFAULT_FILTER, MAX_FILTER_BYTES, filter};
use crate::GatewayError;
use tracing_subscriber::{EnvFilter, filter::LevelFilter};

#[test]
fn gw_u_034_unset_or_empty_keeps_info_with_the_quiet_dependencies() -> Result<(), GatewayError> {
    assert_eq!(DEFAULT_FILTER, "info,bollard=warn,hyper=warn,sqlx=warn");
    for value in [None, Some(""), Some("  ")] {
        let chosen = filter(value)?;
        assert_eq!(
            chosen.max_level_hint(),
            Some(LevelFilter::INFO),
            "{value:?}: nothing above info"
        );
        assert_eq!(
            chosen.to_string(),
            EnvFilter::new(DEFAULT_FILTER).to_string(),
            "{value:?}"
        );
    }
    Ok(())
}

#[test]
fn gw_u_034_an_override_is_used_as_written() -> Result<(), GatewayError> {
    let chosen = filter(Some(" info,tqa_gateway::relay=debug "))?;
    assert_eq!(chosen.max_level_hint(), Some(LevelFilter::DEBUG));
    assert_eq!(
        chosen.to_string(),
        EnvFilter::new("info,tqa_gateway::relay=debug").to_string()
    );
    Ok(())
}

#[test]
fn gw_u_034_an_invalid_or_oversized_filter_stops_the_start() -> Result<(), GatewayError> {
    let longest = format!(
        "info,{}=debug",
        "a".repeat(MAX_FILTER_BYTES - "info,=debug".len())
    );
    assert_eq!(
        filter(Some(&longest))?.max_level_hint(),
        Some(LevelFilter::DEBUG),
        "exactly the bound is accepted"
    );
    let long = format!("{longest}a");
    for value in ["info,tqa_gateway=[", "=debug=", long.as_str()] {
        assert!(
            matches!(filter(Some(value)), Err(GatewayError::Configuration(_))),
            "{value}"
        );
    }
    Ok(())
}
