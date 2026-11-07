//! SEC-016/018: external traffic and DNS remain blocked when proxy egress is enabled.

use bollard::{
    models::{ContainerCreateBody, HostConfig},
    query_parameters::CreateContainerOptions,
};
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{EgressMode, SandboxBackend};

#[path = "support/docker.rs"]
mod support;
use support::{TestResult, bounded, run};

// TEST: SEC-018, SEC-006
#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sec_018_proxy_cannot_connect_directly_to_public_or_private_addresses() -> TestResult {
    run(async |fixture| {
        let mut spec = fixture.spec("blocked-direct", false)?;
        spec.egress.mode = EgressMode::Proxy;
        spec.egress.proxy_url = Some("http://gateway:8080".parse()?);
        let handle = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await?;
        let network = fixture.sandbox_network(&handle.id).await?;
        let network = bounded(fixture.docker.inspect_network(&network, None)).await?;
        assert_eq!(network.internal, Some(true));
        let outside = helper(fixture, "bridge", "outside").await?;
        let inspect = bounded(fixture.docker.inspect_container(&outside, None)).await?;
        let private = inspect
            .network_settings
            .and_then(|settings| settings.networks)
            .and_then(|mut networks| networks.remove("bridge"))
            .and_then(|network| network.ip_address)
            .ok_or("missing private peer")?;
        let public = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::net::lookup_host(("example.com", 443)),
        )
        .await??
        .find(std::net::SocketAddr::is_ipv4)
        .ok_or("public DNS has no IPv4 answer")?
        .ip()
        .to_string();
        for (address, port) in [(private.as_str(), "8080"), (public.as_str(), "443")] {
            assert_ne!(
                fixture
                    .exec(&handle.id, &["nc", "-z", "-w", "2", address, port])
                    .await?
                    .0,
                0
            );
        }
        let (exit, routes) = fixture
            .exec(&handle.id, &["cat", "/proc/net/route"])
            .await?;
        assert_eq!(exit, 0);
        assert!(
            !routes
                .lines()
                .skip(1)
                .any(|line| line.split_whitespace().nth(1) == Some("00000000"))
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sec_016_external_dns_fails_while_the_gateway_peer_resolves() -> TestResult {
    run(async |fixture| {
        let mut spec = fixture.spec("dns", false)?;
        spec.egress.mode = EgressMode::Proxy;
        spec.egress.proxy_url = Some("http://gateway:8080".parse()?);
        let handle = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await?;
        // The gateway (the fixture's stand-in) resolves by its alias on the sandbox's own network.
        let (exit, answer) = fixture
            .exec(&handle.id, &["timeout", "5", "host", "-t", "A", "gateway"])
            .await?;
        assert_eq!(exit, 0, "{answer}");
        assert!(answer.contains("gateway"), "{answer}");
        for name in ["example.com", "example.org"] {
            let (exit, answer) = fixture
                .exec(&handle.id, &["timeout", "5", "host", "-t", "A", name])
                .await?;
            assert_ne!(exit, 0, "external DNS resolved: {answer}");
        }
        Ok(())
    })
    .await
}

async fn helper(
    fixture: &support::Fixture,
    network: &str,
    suffix: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let created = bounded(fixture.docker.create_container(
        Some(CreateContainerOptions {
            name: Some(format!("{}-{suffix}", fixture.prefix)),
            ..Default::default()
        }),
        ContainerCreateBody {
            image: Some(format!("{}@{}", support::IMAGE, support::DIGEST)),
            user: Some("1000:1000".into()),
            cmd: Some(vec!["sh".into(), "-c".into(), "nc -L -p 8080 cat".into()]),
            labels: Some(HashMap::from([(
                "tqa.instance".into(),
                fixture.prefix.clone(),
            )])),
            host_config: Some(HostConfig {
                network_mode: Some(network.into()),
                readonly_rootfs: Some(true),
                cap_drop: Some(vec!["ALL".into()]),
                security_opt: Some(vec!["no-new-privileges".into(), "seccomp=builtin".into()]),
                memory: Some(32 * 1024 * 1024),
                memory_swap: Some(32 * 1024 * 1024),
                pids_limit: Some(16),
                ..Default::default()
            }),
            ..Default::default()
        },
    ))
    .await?;
    bounded(fixture.docker.start_container(&created.id, None)).await?;
    // Container start returns before its listener is ready; observe the process with a fixed bound.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while fixture
            .exec(&created.id, &["nc", "-z", "-w", "1", "127.0.0.1", "8080"])
            .await?
            .0
            != 0
        {
            tokio::task::yield_now().await;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await??;
    Ok(created.id)
}
