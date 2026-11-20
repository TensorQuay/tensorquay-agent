//! GW-U-238: `[channels]`: the wake budget is per day (decided on 21 Nov 2026, C-3): 2,000,000 tokens a day by
//! default, under a ceiling of 10,000,000, and the window's length (a day) is a setting so tests can shorten it.
use crate::{GatewayConfig, GatewayError};
use std::{error::Error, time::Duration};

fn config(extra: &str) -> String {
    let image = format!("sha256:{}", "a".repeat(64));
    format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n{extra}"
    )
}

// TEST: GW-U-238
#[test]
fn gw_u_238_the_wake_budget_is_two_million_tokens_a_day_by_default() -> Result<(), Box<dyn Error>> {
    let parsed = GatewayConfig::from_toml(&config(""))?;
    assert_eq!(parsed.channels.wake_budget, 2_000_000);
    assert_eq!(parsed.channels.max_wake_budget, 10_000_000);
    assert_eq!(parsed.channels.budget_window, Duration::from_hours(24));
    let parsed = GatewayConfig::from_toml(&config(
        "[channels]\nwake_budget=100\nbudget_window_seconds=1\n",
    ))?;
    assert_eq!(parsed.channels.wake_budget, 100);
    assert_eq!(parsed.channels.budget_window, Duration::from_secs(1));
    let parsed = GatewayConfig::from_toml(&config("[channels]\nbudget_window_seconds=604800\n"))?;
    assert_eq!(parsed.channels.budget_window, Duration::from_hours(168));
    Ok(())
}

// TEST: GW-U-238
#[test]
fn gw_u_238_a_window_outside_its_bounds_stops_the_gateway() {
    for seconds in [0, 604_801] {
        let refused = match GatewayConfig::from_toml(&config(&format!(
            "[channels]\nbudget_window_seconds={seconds}\n"
        ))) {
            Err(GatewayError::Configuration(reason)) => Some(reason),
            _ => None,
        };
        assert_eq!(
            refused,
            Some("channels.budget_window_seconds must be 1 through 604800"),
            "{seconds}"
        );
    }
}
