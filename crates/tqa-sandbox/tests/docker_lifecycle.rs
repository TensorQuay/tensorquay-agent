//! Docker integration contracts; all resources are scoped and cleaned by the fixture.

use std::time::Duration;

use futures::future::try_join_all;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{ImageRef, SandboxBackend, SandboxError, reconcile};

#[path = "support/docker.rs"]
mod support;
use support::{TestResult, bounded, run};

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_010_local_image_id_is_ready_and_creates_with_the_exact_pin() -> TestResult {
    run(async |fixture| {
        let inspected = bounded(fixture.docker.inspect_image(&format!(
            "{}@{}",
            support::IMAGE,
            support::DIGEST
        )))
        .await?;
        let identity = inspected.id.ok_or("local image ID missing")?;
        let image = ImageRef::local_id(&identity)?;
        assert!(fixture.backend.image_present(&image).await?);
        let mut spec = fixture.spec("local-image", false)?;
        spec.image = image;
        let handle = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await?;
        let container = bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
        assert_eq!(
            container.config.and_then(|config| config.image).as_deref(),
            Some(identity.as_str())
        );
        fixture.backend.destroy(&handle).await?;
        let mut missing = fixture.spec("missing-local-image", false)?;
        missing.image = ImageRef::local_id(format!("sha256:{}", "0".repeat(64)))?;
        assert!(!fixture.backend.image_present(&missing.image).await?);
        assert_eq!(
            fixture
                .backend
                .create(missing, CancellationToken::new())
                .await,
            Err(SandboxError::ImageNotFound)
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_001_address_requires_a_running_owned_container() -> TestResult {
    run(async |fixture| {
        let handle = fixture
            .backend
            .create(fixture.spec("address", false)?, CancellationToken::new())
            .await?;
        let mut foreign = handle.clone();
        foreign.kind = tqa_sandbox::BackendKind::Firecracker;
        assert_eq!(
            fixture.backend.address(&foreign).await,
            Err(SandboxError::NotFound)
        );
        bounded(fixture.docker.stop_container(
            &handle.id,
            Some(bollard::query_parameters::StopContainerOptions {
                t: Some(1),
                ..Default::default()
            }),
        ))
        .await?;
        assert_eq!(
            fixture.backend.address(&handle).await,
            Err(SandboxError::NotFound)
        );
        fixture.backend.destroy(&handle).await?;
        assert_eq!(
            fixture.backend.address(&handle).await,
            Err(SandboxError::NotFound)
        );
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_010_image_presence_reports_local_and_missing_digests_without_creation() -> TestResult
{
    run(async |fixture| {
        let present = ImageRef::new(support::IMAGE, support::DIGEST)?;
        let missing = ImageRef::new(support::IMAGE, format!("sha256:{}", "0".repeat(64)))?;
        assert!(fixture.backend.image_present(&present).await?);
        assert!(!fixture.backend.image_present(&missing).await?);
        assert!(fixture.backend.list().await?.is_empty());
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_001_create_ready_destroy_is_idempotent() -> TestResult {
    run(async |fixture| {
        // The create starts just before a whole second, so the engine creates the container in the next one: the
        // handle carries our clock's second, sampled before the create, and the listing the engine's (SBX-U-021).
        near_the_end_of_a_second().await;
        let started = std::time::Instant::now();
        let handle = fixture
            .backend
            .create(fixture.spec("lifecycle", false)?, CancellationToken::new())
            .await?;
        let took = started.elapsed();
        fixture
            .backend
            .wait_ready(
                &handle,
                &support::Ready,
                Duration::from_secs(3),
                CancellationToken::new(),
            )
            .await?;
        // Listed by what it names (kind, id, conversation), not by its creation time: the listing reads the engine's,
        // the handle carries our clock's second sampled before the engine's create, and with the create started just
        // before a whole second they differ by one. Both fall inside the create call and each is cut to its second, so
        // they are at most the call's time plus 1 s apart (2 s on an idle engine; more on a busy one, which would make
        // a fixed bound flaky).
        let listed = fixture.backend.list().await?;
        let found = listed
            .iter()
            .find(|listed| identity(listed) == identity(&handle))
            .ok_or_else(|| format!("{handle:?} not listed: {listed:?}"))?;
        let apart = found
            .created_at
            .duration_since(handle.created_at)
            .or_else(|_| handle.created_at.duration_since(found.created_at))?;
        assert!(
            apart <= took + Duration::from_secs(1),
            "listed {:?} against created {:?}, the create took {took:?}",
            found.created_at,
            handle.created_at
        );
        let address = fixture.backend.address(&handle).await?;
        assert!(!address.is_unspecified() && !address.is_loopback());
        let mut foreign = handle.clone();
        foreign.conversation_id = "foreign-address-owner".parse()?;
        assert_eq!(
            fixture.backend.address(&foreign).await,
            Err(SandboxError::NotFound)
        );
        fixture.backend.destroy(&handle).await?;
        fixture.backend.destroy(&handle).await?;
        assert!(fixture.backend.list().await?.is_empty());
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_002_sbx_c_015_workspace_persists_for_bind_and_volume() -> TestResult {
    run(async |fixture| {
        for (bind, suffix) in [(false, "volume"), (true, "bind")] {
            let spec = fixture.spec(suffix, bind)?;
            let first = fixture
                .backend
                .create(spec.clone(), CancellationToken::new())
                .await?;
            // Modes a fresh file would not get, so a reset to defaults cannot pass.
            let (exit, _) = fixture
                .exec(
                    &first.id,
                    &[
                        "sh",
                        "-c",
                        "printf persistent > /workspace/persisted && chmod 0640 /workspace/persisted \
                         && mkdir -p /workspace/private && chmod 0701 /workspace/private \
                         && printf run > /workspace/private/tool && chmod 0751 /workspace/private/tool",
                    ],
                )
                .await?;
            assert_eq!(exit, 0);
            let before = ownership(fixture, &first.id).await?;
            // Owned by the sandbox user the backend configured (uid:gid), as written from inside.
            let user = bounded(fixture.docker.inspect_container(&first.id, None))
                .await?
                .config
                .and_then(|config| config.user)
                .ok_or("the sandbox user is not configured")?;
            assert!(
                before
                    .lines()
                    .all(|line| line.starts_with(&format!("{user} "))),
                "{suffix}: written by the sandbox user {user}: {before}"
            );
            let modes = [
                " 640 /workspace/persisted",
                " 701 /workspace/private",
                " 751 /workspace/private/tool",
            ];
            assert!(
                modes.iter().all(|mode| before.contains(mode)),
                "{suffix}: the modes that were set: {before}"
            );
            fixture.backend.destroy(&first).await?;
            let second = fixture
                .backend
                .create(spec, CancellationToken::new())
                .await?;
            assert_eq!(
                fixture
                    .exec(&second.id, &["cat", "/workspace/persisted"])
                    .await?,
                (0, "persistent".into())
            );
            assert_eq!(
                ownership(fixture, &second.id).await?,
                before,
                "{suffix}: ownership and permissions preserved"
            );
            fixture.backend.destroy(&second).await?;
        }
        Ok(())
    })
    .await
}

/// `uid:gid mode path` for each file the persistence test wrote, in a fixed order.
async fn ownership(
    fixture: &support::Fixture,
    id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let (exit, text) = fixture
        .exec(
            id,
            &[
                "stat",
                "-c",
                "%u:%g %a %n",
                "/workspace/persisted",
                "/workspace/private",
                "/workspace/private/tool",
            ],
        )
        .await?;
    assert_eq!(exit, 0, "{text}");
    assert_eq!(text.lines().count(), 3, "{text}");
    Ok(text)
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_003_memory_hog_dies_but_the_sandbox_survives() -> TestResult {
    run(async |fixture| {
        let handle = fixture
            .backend
            .create(fixture.spec("limits", false)?, CancellationToken::new())
            .await?;
        let (exit, _) = fixture
            .exec(
                &handle.id,
                &[
                    "sh",
                    "-c",
                    "value=x; while :; do value=$value$value$value$value; done",
                ],
            )
            .await?;
        assert_eq!(exit, 137, "the allocating child must be OOM-killed");
        assert_eq!(fixture.exec(&handle.id, &["true"]).await?.0, 0);
        let stats = fixture.backend.stats(&handle).await?;
        assert!(stats.memory_bytes <= 64 * 1024 * 1024);
        assert!(stats.pids > 0);
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_008_reconciliation_lists_owned_stopped_containers_and_cleans_orphans() -> TestResult
{
    run(async |fixture| run(async |foreign| reconciliation_inventory(fixture, foreign).await).await)
        .await
}

async fn reconciliation_inventory(
    fixture: &support::Fixture,
    foreign: &support::Fixture,
) -> TestResult {
    let foreign_handle = foreign
        .backend
        .create(foreign.spec("orphan", false)?, CancellationToken::new())
        .await?;
    let mut handles = Vec::new();
    let mut credentials = Vec::new();
    for suffix in ["bound_running", "bound_stopped", "orphan"] {
        let handle = fixture
            .backend
            .create(fixture.spec(suffix, false)?, CancellationToken::new())
            .await?;
        credentials.push(credential_directory(fixture, &handle).await?);
        handles.push(handle);
    }
    for handle in &handles[1..] {
        bounded(fixture.docker.stop_container(
            &handle.id,
            Some(bollard::query_parameters::StopContainerOptions {
                t: Some(1),
                ..Default::default()
            }),
        ))
        .await?;
    }
    // list() reports the engine's creation time and create() its label's, which can be a second apart: shift every
    // created handle by that second, so the comparisons below (by identity) hold whichever second each lands in.
    let mut foreign_handle = foreign_handle;
    for handle in handles
        .iter_mut()
        .chain(std::iter::once(&mut foreign_handle))
    {
        handle.created_at += Duration::from_secs(1);
    }
    let listed = fixture.backend.list().await?;
    assert_eq!(listed.len(), 3);
    let listed_ids = identities(&listed);
    assert!(
        handles
            .iter()
            .all(|handle| listed_ids.contains(&identity(handle)))
    );
    assert!(!listed_ids.contains(&identity(&foreign_handle)));
    let reconciliation = reconcile(&handles[..2], &listed);
    assert!(reconciliation.missing.is_empty());
    assert_eq!(
        identities(&reconciliation.orphans),
        vec![identity(&handles[2])]
    );
    fixture.backend.destroy(&reconciliation.orphans[0]).await?;
    assert!(!credentials[2].exists(), "orphan token directory survives");
    let remaining = fixture.backend.list().await?;
    let remaining = identities(&remaining);
    assert_eq!(remaining.len(), 2);
    for (handle, directory) in handles[..2].iter().zip(&credentials) {
        assert!(remaining.contains(&identity(handle)));
        assert!(directory.join("token").is_file());
    }
    let foreign_listed = foreign.backend.list().await?;
    assert_eq!(identities(&foreign_listed), vec![identity(&foreign_handle)]);
    Ok(())
}

/// What a handle names, without its creation time: `list()` reads the engine's own time and `create()` its label's,
/// which can be a second apart, and nothing matches handles by time (reconcile and teardown go by id, recovery by
/// conversation).
fn identity(
    handle: &tqa_sandbox::SandboxHandle,
) -> (tqa_sandbox::BackendKind, &str, &tqa_types::ConversationId) {
    (handle.kind, handle.id.as_str(), &handle.conversation_id)
}

fn identities(
    handles: &[tqa_sandbox::SandboxHandle],
) -> Vec<(tqa_sandbox::BackendKind, &str, &tqa_types::ConversationId)> {
    handles.iter().map(identity).collect()
}

async fn credential_directory(
    fixture: &support::Fixture,
    handle: &tqa_sandbox::SandboxHandle,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let inspected = bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
    let token_id = inspected
        .config
        .and_then(|config| config.labels)
        .and_then(|labels| labels.get("tqa.token_id").cloned())
        .ok_or("missing token directory label")?;
    let directory = fixture.storage.path().join("sandboxes").join(token_id);
    assert!(directory.join("token").is_file());
    Ok(directory)
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_010_unknown_digest_leaves_no_container_network_volume_or_process() -> TestResult {
    run(async |fixture| {
        let mut spec = fixture.spec("missing", false)?;
        spec.image = ImageRef::new(support::IMAGE, format!("sha256:{}", "0".repeat(64)))?;
        assert!(matches!(
            fixture.backend.create(spec, CancellationToken::new()).await,
            Err(SandboxError::ImageNotFound)
        ));
        assert!(fixture.backend.list().await?.is_empty());
        let network = bounded(fixture.docker.inspect_network(&fixture.prefix, None)).await;
        assert!(
            network.is_err(),
            "unknown images must fail before network creation"
        );
        assert_eq!(fixture.leftovers().await?, Vec::<String>::new());
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_011_sixteen_concurrent_creates_have_unique_handles_and_addresses() -> TestResult {
    run(async |fixture| {
        let specifications = (0..16)
            .map(|index| fixture.spec(&format!("parallel_{index}"), false))
            .collect::<Result<Vec<_>, _>>()?;
        let handles = try_join_all(
            specifications
                .into_iter()
                .map(|spec| fixture.backend.create(spec, CancellationToken::new())),
        )
        .await?;
        let identities = handles
            .iter()
            .map(|handle| &handle.id)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(identities.len(), 16);
        for handle in &handles {
            fixture
                .backend
                .wait_ready(
                    handle,
                    &support::Ready,
                    Duration::from_secs(3),
                    CancellationToken::new(),
                )
                .await?;
        }
        let mut addresses = std::collections::HashSet::new();
        let mut token_directories = std::collections::HashSet::new();
        for handle in &handles {
            let inspected = bounded(fixture.docker.inspect_container(&handle.id, None)).await?;
            let address = fixture.sandbox_address(&handle.id).await?;
            addresses.insert(address);
            token_directories.insert(
                inspected
                    .config
                    .and_then(|config| config.labels)
                    .and_then(|labels| labels.get("tqa.token_id").cloned())
                    .ok_or("missing token directory label")?,
            );
        }
        assert_eq!(addresses.len(), 16, "unique addresses: {addresses:?}");
        assert_eq!(token_directories.len(), 16, "unique token directories");
        Ok(())
    })
    .await
}

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_013_destroy_terminates_an_active_process() -> TestResult {
    run(async |fixture| {
        let handle = fixture
            .backend
            .create(fixture.spec("busy", false)?, CancellationToken::new())
            .await?;
        let active = bounded(fixture.docker.create_exec(
            &handle.id,
            bollard::exec::CreateExecOptions {
                cmd: Some(vec!["sleep", "600"]),
                ..Default::default()
            },
        ))
        .await?;
        bounded(fixture.docker.start_exec(
            &active.id,
            Some(bollard::exec::StartExecOptions {
                detach: true,
                ..Default::default()
            }),
        ))
        .await?;
        fixture.backend.destroy(&handle).await?;
        assert!(
            bounded(fixture.docker.inspect_container(&handle.id, None))
                .await
                .is_err()
        );
        Ok(())
    })
    .await
}

/// Waits until the wall clock is at least 0.93 s into its current second.
async fn near_the_end_of_a_second() {
    loop {
        let into = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map_or(0, |now| now.subsec_millis());
        if into >= 930 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(u64::from(930 - into))).await;
    }
}
