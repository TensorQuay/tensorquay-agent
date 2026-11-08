//! GW-U-052: the live workspace quota's settings: defaults and bounds.
use crate::{GatewayConfig, GatewayError};
use std::{error::Error, time::Duration};

fn config(extra: &str) -> String {
    let image = format!("sha256:{}", "a".repeat(64));
    format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n{extra}"
    )
}

fn refused(toml: &str) -> Option<&'static str> {
    match GatewayConfig::from_toml(toml) {
        Err(GatewayError::Configuration(reason)) => Some(reason),
        _ => None,
    }
}

// TEST: GW-U-052
#[test]
fn gw_u_052_the_live_check_is_on_every_30_s_with_a_10_percent_margin_by_default()
-> Result<(), Box<dyn Error>> {
    let parsed = GatewayConfig::from_toml(&config(""))?;
    assert_eq!(parsed.workspaces.live_check, Some(Duration::from_secs(30)));
    assert_eq!(parsed.workspaces.margin_percent, 10);
    assert_eq!(parsed.sandbox.state_bytes, 1 << 30);
    let parsed = GatewayConfig::from_toml(&config(
        "[workspaces]\nlive_check=false\ncheck_seconds=5\nmargin_percent=100\n[sandbox]\nstate_bytes=16777216\n",
    ))?;
    assert_eq!(parsed.workspaces.live_check, None);
    assert_eq!(parsed.workspaces.margin_percent, 100);
    assert_eq!(parsed.sandbox.state_bytes, 16 << 20);
    Ok(())
}

// TEST: GW-U-052
#[test]
fn gw_u_052_settings_outside_their_bounds_stop_the_gateway() {
    for (extra, reason) in [
        (
            "[workspaces]\ncheck_seconds=4\n",
            "workspaces.check_seconds must be 5 through 600",
        ),
        (
            "[workspaces]\ncheck_seconds=601\n",
            "workspaces.check_seconds must be 5 through 600",
        ),
        (
            "[workspaces]\nmargin_percent=0\n",
            "workspaces.margin_percent must be 1 through 100",
        ),
        (
            "[workspaces]\nmargin_percent=101\n",
            "workspaces.margin_percent must be 1 through 100",
        ),
        (
            "[sandbox]\nstate_bytes=16777215\n",
            "sandbox.state_bytes must be 16 MiB through 1 TiB",
        ),
        (
            "[sandbox]\nstate_bytes=1099511627777\n",
            "sandbox.state_bytes must be 16 MiB through 1 TiB",
        ),
    ] {
        assert_eq!(refused(&config(extra)), Some(reason), "{extra}");
    }
}
