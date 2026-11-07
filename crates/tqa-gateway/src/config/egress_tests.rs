//! GW-U-113 (guardrails §1.4): the proxy's bounds are configuration with documented defaults, each within its range.
use super::{EgressInput, egress};
use std::{net::SocketAddr, time::Duration};

const PUBLIC: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 8080);

fn section(text: &str) -> Result<EgressInput, toml::de::Error> {
    toml::from_str(text)
}

// TEST: GW-U-113
#[test]
fn gw_u_113_the_proxys_bounds_have_defaults_and_ranges() -> Result<(), Box<dyn std::error::Error>> {
    // The defaults, with no section and with an empty one.
    for config in [egress(None, PUBLIC)?, egress(Some(section("")?), PUBLIC)?] {
        assert_eq!(
            (
                config.connections,
                config.idle,
                config.keyed_connections,
                config.keyed_per_sandbox,
                config.keyed_answer
            ),
            (256, Duration::from_mins(5), 256, 16, Duration::from_mins(2))
        );
    }
    let set = egress(
        Some(section(
            "connections = 64\nidle_seconds = 30\nkeyed_connections = 32\nkeyed_per_sandbox = 4\nkeyed_answer_seconds = 10",
        )?),
        PUBLIC,
    )?;
    assert_eq!(
        (
            set.connections,
            set.idle,
            set.keyed_connections,
            set.keyed_per_sandbox,
            set.keyed_answer
        ),
        (64, Duration::from_secs(30), 32, 4, Duration::from_secs(10))
    );
    // Each out of its range, and a sandbox's bound above the total, refuse the file.
    for text in [
        "connections = 0",
        "connections = 4097",
        "idle_seconds = 0",
        "idle_seconds = 86401",
        "keyed_connections = 0",
        "keyed_connections = 4097",
        "keyed_per_sandbox = 0",
        "keyed_per_sandbox = 257",
        "keyed_connections = 8\nkeyed_per_sandbox = 9",
        "keyed_answer_seconds = 0",
        "keyed_answer_seconds = 3601",
    ] {
        assert!(egress(Some(section(text)?), PUBLIC).is_err(), "{text}");
    }
    Ok(())
}
