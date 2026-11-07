//! GW-U-040: the Docker backend settings the gateway derives from its configuration.
use std::error::Error;

use super::{docker_options, startup_refusal};
use crate::GatewayConfig;

#[test]
fn gw_u_040_operations_follow_io_timeout_and_teardown_keeps_its_longer_bound()
-> Result<(), Box<dyn Error>> {
    let image = format!("sha256:{}", "a".repeat(64));
    let config = GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\ngateway_container='tqa-gateway'\n"
    ))?;
    let options = docker_options(&config)?;
    assert_eq!(options.operation_timeout, config.io_timeout());
    // A removal on a loaded daemon may take longer than an ordinary operation; cutting it short reports a retire as
    // failed although the container goes (seen on the real stack: a successful retirement turned into a failed conversation).
    assert_eq!(
        options.destroy_timeout,
        tqa_sandbox::docker::DEFAULT_DESTROY_TIMEOUT
    );
    assert!(options.destroy_timeout > config.io_timeout());
    // Setting a sandbox up keeps its own, longer bound too: a loaded daemon's create and start took past io_timeout
    // and failed runs as 'sandbox create failed: deadline exceeded' (SBX-U-009).
    assert_eq!(
        options.create_timeout,
        tqa_sandbox::docker::DEFAULT_CREATE_TIMEOUT
    );
    assert!(options.create_timeout > config.io_timeout());
    Ok(())
}

fn docker(sandbox: &str) -> Result<GatewayConfig, Box<dyn Error>> {
    let image = format!("sha256:{}", "a".repeat(64));
    Ok(GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n{sandbox}"
    ))?)
}

/// An install upgraded from before per-sandbox networks has no `gateway_container`: the gateway stops at start-up
/// with an error naming the key, before it connects to Docker; the development mode, which has no such networks, starts.
#[test]
fn gw_u_040_per_sandbox_networks_without_a_gateway_container_stop_the_start_naming_the_key()
-> Result<(), Box<dyn Error>> {
    let refused = docker_options(&docker("")?).err();
    let Some(crate::GatewayError::Configuration(reason)) = refused else {
        return Err(format!("expected a configuration error, got {refused:?}").into());
    };
    assert!(reason.contains("[sandbox] gateway_container"), "{reason}");
    docker_options(&docker("network_mode='dev_host_gateway'\n")?)?;
    Ok(())
}

// TEST: GW-U-122
/// One start's container create and start, retries included, share `[sandbox] create_budget_seconds`: a person sees
/// "Starting…" at most that long before a start fails (SBX-U-014).
#[test]
fn gw_u_122_the_create_budget_defaults_to_two_minutes_and_reaches_the_backend()
-> Result<(), Box<dyn Error>> {
    let config = docker("gateway_container='tqa-gateway'\n")?;
    assert_eq!(
        docker_options(&config)?.create_budget,
        std::time::Duration::from_mins(2)
    );
    let config = docker("gateway_container='tqa-gateway'\ncreate_budget_seconds=90\n")?;
    let options = docker_options(&config)?;
    assert_eq!(options.create_budget, std::time::Duration::from_secs(90));
    // Room for a retry: the budget is longer than one create's own bound.
    assert!(options.create_budget > options.create_timeout);
    Ok(())
}

// TEST: GW-U-122
/// A budget below one container create's own bound (60 s) would cut even the first create short: on a loaded host
/// (44 s creates) every start would fail. It is refused at load, naming the setting and the bound.
#[test]
fn gw_u_122_a_create_budget_out_of_range_is_refused_naming_the_setting()
-> Result<(), Box<dyn Error>> {
    for value in [0, 29, 30, 59, 601] {
        let refused = docker(&format!(
            "gateway_container='tqa-gateway'\ncreate_budget_seconds={value}\n"
        ))
        .err()
        .map(|error| error.to_string());
        assert!(
            refused.as_deref().is_some_and(
                |reason| reason.contains("create_budget_seconds") && reason.contains("60")
            ),
            "{value}: {refused:?}"
        );
    }
    let config = docker("gateway_container='tqa-gateway'\ncreate_budget_seconds=60\n")?;
    assert_eq!(
        docker_options(&config)?.create_budget,
        tqa_sandbox::docker::DEFAULT_CREATE_TIMEOUT
    );
    Ok(())
}

// TEST: GW-U-123
#[test]
fn gw_u_123_the_startup_budget_defaults_to_80_s_and_is_range_checked() -> Result<(), Box<dyn Error>>
{
    let config = docker("gateway_container='tqa-gateway'\n")?;
    assert_eq!(
        config.sandbox.startup_budget(),
        std::time::Duration::from_secs(80)
    );
    let config = docker("gateway_container='tqa-gateway'\nstartup_budget_seconds=75\n")?;
    assert_eq!(
        config.sandbox.startup_budget(),
        std::time::Duration::from_secs(75)
    );
    // At least one full container create (60 s); at most ten minutes.
    for value in [0, 59, 601] {
        let refused = docker(&format!(
            "gateway_container='tqa-gateway'\nstartup_budget_seconds={value}\n"
        ))
        .err()
        .map(|error| error.to_string());
        assert!(
            refused
                .as_deref()
                .is_some_and(|reason| reason.contains("startup_budget_seconds")),
            "{value}: {refused:?}"
        );
    }
    Ok(())
}

// TEST: GW-U-123
#[test]
fn gw_u_123_an_engine_that_never_answers_at_start_up_is_refused_in_plain_words() {
    let refused = startup_refusal(tqa_sandbox::SandboxError::Timeout).to_string();
    assert!(
        refused.contains("the container engine did not answer in time while the gateway started"),
        "{refused}"
    );
    assert!(
        refused.contains("start the gateway again when the host is less busy"),
        "{refused}"
    );
    // Any other failure keeps its own cause.
    assert_eq!(
        startup_refusal(tqa_sandbox::SandboxError::Preflight("kvm is missing")).to_string(),
        "sandbox configure failed: backend preflight failed"
    );
}
