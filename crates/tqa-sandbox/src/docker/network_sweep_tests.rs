//! SBX-U-006: reconciling sandbox networks (ADR-0021 amendment) against a scripted engine: the gateway rejoins live
//! sandboxes' networks at start, orphans are removed with their subnets returned, and sandboxes on the former shared
//! network are reported.
use super::{
    NetworkMode, NetworkSweep,
    sandbox_network_tests::{TestResult, free, inspected, scripted, scripted_backend, step},
};

/// The owned containers as the engine lists them: one on its own network, one on the former shared network.
fn containers() -> String {
    r#"[
        {"Id":"c1","Labels":{"tqa.instance":"tqa-unit","tqa.conversation":"conversation_a","tqa.created_at":"1","tqa.network":"tqa-sbx-1"}},
        {"Id":"c2","Labels":{"tqa.instance":"tqa-unit","tqa.conversation":"conversation_b","tqa.created_at":"2"}}
    ]"#
    .into()
}

const NETWORKS: &str = r#"[{"Name":"tqa-sbx-2"},{"Name":"tqa-sbx-1"}]"#;

#[tokio::test]
async fn sbx_u_006_at_start_the_gateway_rejoins_live_networks_and_orphans_are_removed() -> TestResult
{
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    backend
        .allocator
        .lock()
        .map_err(|_| "allocator")?
        .allocate();
    let (sweep, seen) = scripted(
        listener,
        vec![
            step("GET /containers/json", "200 OK", &containers()),
            step("GET /networks", "200 OK", NETWORKS),
            // Sorted: the live network first. The gateway is already on it, which is fine.
            step(
                "/networks/tqa-sbx-1/connect",
                "403 Forbidden",
                r#"{"message":"endpoint with name tqa-gateway already exists in network tqa-sbx-1"}"#,
            ),
            step("GET /networks/tqa-sbx-2", "200 OK", &inspected("tqa-unit", "192.168.4.0/28", true)),
            step("/networks/tqa-sbx-2/disconnect", "200 OK", ""),
            step("DELETE /networks/tqa-sbx-2", "204 No Content", ""),
        ],
        Box::pin(backend.reconcile_networks(true)),
    )
    .await?;
    let sweep = sweep?;
    assert_eq!((sweep.reattached, sweep.removed, sweep.failed), (1, 1, 0));
    assert_eq!(sweep.legacy.len(), 1);
    assert_eq!(sweep.legacy[0].id, "c2");
    assert!(
        seen[1].contains("tqa.subnet"),
        "only labelled sandbox networks: {seen:?}"
    );
    assert_eq!(free(&backend)?, 2, "the orphan's subnet is back");
    assert_eq!(backend.pool_free()?, 2);
    Ok(())
}

#[tokio::test]
async fn sbx_u_006_the_periodic_pass_only_removes_orphans_and_keeps_a_stuck_ones_subnet()
-> TestResult {
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    backend
        .allocator
        .lock()
        .map_err(|_| "allocator")?
        .allocate();
    let (sweep, _) = scripted(
        listener,
        vec![
            step("GET /containers/json", "200 OK", &containers()),
            step("GET /networks", "200 OK", NETWORKS),
            step(
                "GET /networks/tqa-sbx-2",
                "200 OK",
                &inspected("tqa-unit", "192.168.4.0/28", false),
            ),
            step(
                "/networks/tqa-sbx-2/disconnect",
                "403 Forbidden",
                r#"{"message":"not connected"}"#,
            ),
            step(
                "DELETE /networks/tqa-sbx-2",
                "403 Forbidden",
                r#"{"message":"network has active endpoints"}"#,
            ),
        ],
        Box::pin(backend.reconcile_networks(false)),
    )
    .await?;
    assert_eq!(
        sweep?,
        NetworkSweep {
            reattached: 0,
            removed: 0,
            failed: 1,
            legacy: sweep_legacy()?,
        }
    );
    assert_eq!(
        free(&backend)?,
        1,
        "a network that is still there keeps its subnet"
    );
    Ok(())
}

#[tokio::test]
async fn sbx_u_006_a_failed_rejoin_is_counted_and_listing_failures_are_returned() -> TestResult {
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    let (sweep, _) = scripted(
        listener,
        vec![
            step("GET /containers/json", "200 OK", &containers()),
            step("GET /networks", "200 OK", r#"[{"Name":"tqa-sbx-1"}]"#),
            step(
                "/networks/tqa-sbx-1/connect",
                "500 Internal Server Error",
                r#"{"message":"boom"}"#,
            ),
        ],
        Box::pin(backend.reconcile_networks(true)),
    )
    .await?;
    assert_eq!(sweep?.failed, 1);
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    let (sweep, _) = scripted(
        listener,
        vec![step(
            "GET /containers/json",
            "500 Internal Server Error",
            r#"{"message":"boom"}"#,
        )],
        Box::pin(backend.reconcile_networks(true)),
    )
    .await?;
    assert!(sweep.is_err());
    Ok(())
}

#[tokio::test]
async fn sbx_u_006_the_development_mode_has_nothing_to_reconcile() -> TestResult {
    let (mut backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    backend.config.network = NetworkMode::DevHostGateway;
    let (sweep, seen) =
        scripted(listener, vec![], Box::pin(backend.reconcile_networks(true))).await?;
    assert_eq!(sweep?, NetworkSweep::default());
    assert!(seen.is_empty());
    Ok(())
}

/// The legacy sandbox the scripted listing reports.
fn sweep_legacy() -> Result<Vec<crate::SandboxHandle>, Box<dyn std::error::Error>> {
    Ok(vec![crate::SandboxHandle {
        kind: crate::BackendKind::Docker,
        id: "c2".into(),
        conversation_id: "conversation_b".parse()?,
        created_at: std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2),
    }])
}
