//! Per-sandbox networks on Docker (ADR-0021 amendment). SBX-C-011 "no cross-talk": sandboxes cannot reach each other;
//! the gateway is the only peer a sandbox can reach (ARCHITECTURE §4.2). SBX-D-007: a sandbox's network is its own,
//! internal, from the pool, and goes with it. SBX-D-008: reconciliation after a crash or a gateway restart.

use std::time::Duration;

use bollard::models::NetworkDisconnectRequest;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::SandboxBackend;

#[path = "support/docker.rs"]
mod support;
use support::{TestResult, bounded, run};

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_011_one_sandbox_cannot_reach_another() -> TestResult {
    run(async |fixture| {
        let mut handles = Vec::new();
        for suffix in ["talker", "listener"] {
            let handle = fixture
                .backend
                .create(fixture.spec(suffix, false)?, CancellationToken::new())
                .await?;
            fixture
                .backend
                .wait_ready(
                    &handle,
                    &support::Ready,
                    Duration::from_secs(3),
                    CancellationToken::new(),
                )
                .await?;
            handles.push(handle);
        }
        cross_talk(fixture, &handles[0].id, &handles[1].id).await
    })
    .await
}

/// A listener in one sandbox is reachable from that sandbox's own address but not from another sandbox: the gateway
/// is the only peer a sandbox can reach (ARCHITECTURE §4.2).
async fn cross_talk(fixture: &support::Fixture, from: &str, to: &str) -> TestResult {
    let address = fixture.sandbox_address(to).await?;
    let (exit, text) = fixture
        .exec(
            to,
            &["sh", "-c", "nc -L -p 8080 cat </dev/null >/dev/null 2>&1 &"],
        )
        .await?;
    assert_eq!(exit, 0, "{text}");
    // The listener is up and bound to the sandbox's network address (checked from inside; each probe is one exec
    // round trip, so no sleep is needed between attempts).
    let mut listening = false;
    for _ in 0..50 {
        if fixture
            .exec(to, &["nc", "-z", "-w", "1", &address, "8080"])
            .await?
            .0
            == 0
        {
            listening = true;
            break;
        }
    }
    assert!(listening, "the control listener never came up");
    // Positive control: the gateway, the only peer, is reachable from the other sandbox.
    assert_eq!(
        fixture
            .exec(from, &["nc", "-z", "-w", "2", "gateway", "8080"])
            .await?
            .0,
        0,
        "the gateway is reachable"
    );
    assert_ne!(
        fixture
            .exec(from, &["nc", "-z", "-w", "2", &address, "8080"])
            .await?
            .0,
        0,
        "one sandbox reached another sandbox's listener"
    );
    Ok(())
}

async fn ready(
    fixture: &support::Fixture,
    suffix: &str,
) -> Result<tqa_sandbox::SandboxHandle, Box<dyn std::error::Error>> {
    let handle = fixture
        .backend
        .create(fixture.spec(suffix, false)?, CancellationToken::new())
        .await?;
    fixture
        .backend
        .wait_ready(
            &handle,
            &support::Ready,
            Duration::from_secs(3),
            CancellationToken::new(),
        )
        .await?;
    Ok(handle)
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_d_007_a_sandbox_network_is_its_own_internal_pool_network_and_goes_with_it()
-> TestResult {
    run(async |fixture| {
        let free = fixture.backend.pool_free()?;
        let handle = ready(fixture, "own").await?;
        let name = fixture.sandbox_network(&handle.id).await?;
        let network = bounded(fixture.docker.inspect_network(&name, None)).await?;
        assert_eq!(network.internal, Some(true));
        assert_eq!(network.enable_ipv6, Some(false));
        let subnet = network
            .ipam
            .and_then(|ipam| ipam.config)
            .and_then(|configs| configs.into_iter().next())
            .and_then(|config| config.subnet)
            .ok_or("no subnet")?;
        assert!(
            subnet.starts_with("10.213.") && subnet.ends_with("/29"),
            "{subnet}"
        );
        let mut members: Vec<String> = network
            .containers
            .unwrap_or_default()
            .into_values()
            .filter_map(|container| container.name)
            .collect();
        members.sort();
        let gateway = format!("{}-gateway", fixture.prefix);
        let sandbox = bounded(fixture.docker.inspect_container(&handle.id, None))
            .await?
            .name
            .map(|name| name.trim_start_matches('/').to_owned())
            .ok_or("sandbox name")?;
        let mut expected = vec![gateway, sandbox];
        expected.sort();
        assert_eq!(members, expected, "only the sandbox and the gateway");
        assert_eq!(fixture.backend.pool_free()?, free - 1);
        fixture.backend.destroy(&handle).await?;
        assert_eq!(fixture.leftovers().await?, Vec::<String>::new());
        assert_eq!(fixture.backend.pool_free()?, free, "the subnet is back");
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_d_008_an_orphaned_network_is_removed_and_a_restarted_gateway_rejoins() -> TestResult {
    run(async |fixture| {
        let free = fixture.backend.pool_free()?;
        // An orphan: the sandbox's container vanished behind the backend's back (a crash mid-destroy).
        let orphan = ready(fixture, "orphan").await?;
        let orphan_network = fixture.sandbox_network(&orphan.id).await?;
        bounded(fixture.docker.remove_container(
            &orphan.id,
            Some(bollard::query_parameters::RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        ))
        .await?;
        // A live sandbox whose network lost the gateway (a new gateway container after a restart).
        let live = ready(fixture, "live").await?;
        let live_network = fixture.sandbox_network(&live.id).await?;
        bounded(fixture.docker.disconnect_network(
            &live_network,
            NetworkDisconnectRequest {
                container: format!("{}-gateway", fixture.prefix),
                force: Some(true),
            },
        ))
        .await?;
        assert_ne!(
            fixture
                .exec(&live.id, &["nc", "-z", "-w", "2", "gateway", "8080"])
                .await?
                .0,
            0,
            "the gateway is gone from the network"
        );
        let sweep = fixture.backend.reconcile_networks(true).await?;
        assert_eq!(
            (sweep.reattached, sweep.removed, sweep.failed),
            (1, 1, 0),
            "{sweep:?}"
        );
        assert!(sweep.legacy.is_empty());
        assert!(
            bounded(fixture.docker.inspect_network(&orphan_network, None))
                .await
                .is_err(),
            "the orphan network is removed"
        );
        assert_eq!(
            fixture.backend.pool_free()?,
            free - 1,
            "only the live sandbox holds a subnet"
        );
        assert_eq!(
            fixture
                .exec(&live.id, &["nc", "-z", "-w", "2", "gateway", "8080"])
                .await?
                .0,
            0,
            "the gateway is reachable again"
        );
        fixture.backend.destroy(&live).await?;
        // The orphan's token directory is the container reconciliation's to remove (SBX-C-008), not the sweep's.
        let networks: Vec<String> = fixture
            .leftovers()
            .await?
            .into_iter()
            .filter(|leftover| !leftover.starts_with("token directory"))
            .collect();
        assert_eq!(networks, Vec::<String>::new());
        Ok(())
    })
    .await
}
