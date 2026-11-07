//! GW-U-040: the per-sandbox network settings (ADR-0021 amendment): the subnet pool and the gateway's container name.
use std::error::Error;

use crate::{GatewayConfig, GatewayError};

fn config(sandbox: &str) -> String {
    let image = format!("sha256:{}", "a".repeat(64));
    format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{sandbox}"
    )
}

fn refused(toml: &str) -> Option<&'static str> {
    match GatewayConfig::from_toml(toml) {
        Err(GatewayError::Configuration(reason)) => Some(reason),
        _ => None,
    }
}

/// The TOML parse's refusal (GW-U-144): the field's path and why.
fn field_refused(toml: &str) -> Option<String> {
    match GatewayConfig::from_toml(toml) {
        Err(GatewayError::ConfigurationField(reason)) => Some(reason),
        _ => None,
    }
}

#[test]
fn gw_u_040_the_pool_defaults_to_a_private_slash_16_and_the_gateway_container_is_optional()
-> Result<(), Box<dyn Error>> {
    let parsed = GatewayConfig::from_toml(&config(""))?;
    assert_eq!(
        parsed.sandbox.network_pool,
        tqa_sandbox::docker::SubnetPool::default()
    );
    assert_eq!(parsed.sandbox.gateway_container, None);
    let parsed = GatewayConfig::from_toml(&config(
        "gateway_container='tqa-gateway'\n[sandbox.network_pool]\ncidr='192.168.4.0/24'\nprefix=29\n",
    ))?;
    assert_eq!(parsed.sandbox.network_pool.capacity(), 32);
    assert_eq!(
        parsed.sandbox.gateway_container.as_deref(),
        Some("tqa-gateway")
    );
    Ok(())
}

#[test]
fn gw_u_040_a_bad_pool_or_container_name_stops_the_gateway() {
    for (sandbox, reason) in [
        (
            "gateway_container='-gateway'\n",
            "invalid sandbox gateway_container",
        ),
        (
            "gateway_container=''\n",
            "invalid sandbox gateway_container",
        ),
    ] {
        assert_eq!(refused(&config(sandbox)), Some(reason), "{sandbox}");
    }
    // The pool's own refusals come from its type, so they are the TOML parse's, naming the field (GW-U-144).
    for (sandbox, reason) in [
        (
            "[sandbox.network_pool]\ncidr='203.0.113.0/24'\nprefix=28\n",
            "sandbox.network_pool: invalid value",
        ),
        (
            "[sandbox.network_pool]\ncidr='192.168.4.0/24'\nprefix=30\n",
            "sandbox.network_pool: invalid value",
        ),
        (
            "[sandbox.network_pool]\ncidr='192.168.4.0/24'\nprefix=28\nextra=1\n",
            "sandbox.network_pool.extra: unknown field, expected `cidr` or `prefix`",
        ),
    ] {
        assert_eq!(
            field_refused(&config(sandbox)).as_deref(),
            Some(reason),
            "{sandbox}"
        );
    }
}

#[test]
fn gw_u_040_the_pool_must_hold_twice_max_live() -> Result<(), Box<dyn Error>> {
    // A /24 of /29s holds 32 subnets: 16 live sandboxes fit, 17 do not.
    let pool = "[sandbox.network_pool]\ncidr='192.168.4.0/24'\nprefix=29\n";
    GatewayConfig::from_toml(&config(&format!("max_live=16\n{pool}")))?;
    assert_eq!(
        refused(&config(&format!("max_live=17\n{pool}"))),
        Some("sandbox network_pool holds fewer than twice max_live subnets")
    );
    // Only Docker's per-sandbox networks use the pool: the development mode shares one network, and microVMs are
    // isolated by their own rules.
    GatewayConfig::from_toml(&config(&format!(
        "max_live=17\nnetwork_mode='dev_host_gateway'\n{pool}"
    )))?;
    let firecracker = config(&format!("max_live=17\n{pool}"))
        .replace("backend='docker'", "backend='firecracker'")
        + &format!(
            "[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\n",
            "b".repeat(64)
        );
    GatewayConfig::from_toml(&firecracker)?;
    Ok(())
}
