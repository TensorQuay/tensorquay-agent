//! SBX-U-009: creating and starting a sandbox's container has its own bound, `create_timeout`, longer than the
//! per-operation one: a loaded daemon can take longer than `operation_timeout` to create and start a container (its
//! network namespace, mounts and veth), and cutting that short failed runs as `sandbox create failed: deadline
//! exceeded`. Past `create_timeout` the create still fails, and the half-created container is removed.
//!
//! SBX-U-014: a create past its bound is retried inside the start's create budget, by the same name: a create the
//! daemon finishes late (44 s at I/O full ~50 % on a loaded host) is found and removed, or collides (409), instead of
//! being left behind.
use super::{TestResult, scripted, scripted_backend, slow, step};
use crate::SandboxError;
use bollard::models::ContainerCreateBody;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const CREATED: &str = r#"{"Id":"c1","Warnings":[]}"#;

// TEST: SBX-U-009
#[tokio::test]
async fn sbx_u_009_a_slow_start_within_the_create_bound_succeeds() -> TestResult {
    let (mut backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    backend.config.operation_timeout = Duration::from_millis(200);
    backend.config.create_timeout = Duration::from_secs(3);
    let cancel = CancellationToken::new();
    let (started, seen) = scripted(
        listener,
        vec![
            slow(
                step("POST /containers/create", "201 Created", CREATED),
                Duration::from_millis(400),
            ),
            slow(
                step("POST /containers/c1/start", "204 No Content", ""),
                Duration::from_millis(600),
            ),
        ],
        Box::pin(backend.start_sandbox(
            ContainerCreateBody::default(),
            "tqa-unit-c",
            &cancel,
            backend.config.create_timeout,
        )),
    )
    .await?;
    assert_eq!(started?, "c1", "{seen:?}");
    Ok(())
}

// TEST: SBX-U-009
#[tokio::test]
async fn sbx_u_009_a_start_past_the_create_bound_fails_and_removes_the_container() -> TestResult {
    let (mut backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    backend.config.operation_timeout = Duration::from_millis(200);
    backend.config.create_timeout = Duration::from_millis(400);
    let cancel = CancellationToken::new();
    let (started, seen) = scripted(
        listener,
        vec![
            step("POST /containers/create", "201 Created", CREATED),
            slow(
                step("POST /containers/c1/start", "204 No Content", ""),
                Duration::from_millis(1500),
            ),
            step("DELETE /containers/c1", "204 No Content", ""),
        ],
        Box::pin(backend.start_sandbox(
            ContainerCreateBody::default(),
            "tqa-unit-c",
            &cancel,
            backend.config.create_timeout,
        )),
    )
    .await?;
    assert!(
        matches!(started, Err(SandboxError::Timeout)),
        "{started:?} {seen:?}"
    );
    assert!(
        seen.iter()
            .any(|line| line.starts_with("DELETE /containers/c1")),
        "{seen:?}"
    );
    Ok(())
}

/// The container a timed-out create left: this instance's, under the start's name.
const LATE: &str =
    r#"{"Id":"late","Name":"/tqa-unit-c","Config":{"Labels":{"tqa.instance":"tqa-unit"}}}"#;
const NO_SUCH: &str = r#"{"message":"No such container: tqa-unit-c"}"#;

/// A backend whose creates are bounded by `bound` inside a create budget of `budget`, retried at once.
async fn retrying(
    bound: Duration,
    budget: Duration,
) -> Result<
    (
        super::DockerBackend,
        tokio::net::TcpListener,
        tempfile::TempDir,
    ),
    Box<dyn std::error::Error>,
> {
    let (mut backend, listener, storage) = scripted_backend(Some("tqa-gateway")).await?;
    backend.config.create_timeout = bound;
    backend.config.create_budget = budget;
    backend.retry = crate::retry::Retry::new(3, Duration::from_millis(1));
    Ok((backend, listener, storage))
}

fn creates(seen: &[String]) -> usize {
    seen.iter()
        .filter(|line| line.contains("/containers/create"))
        .count()
}

// TEST: SBX-U-014
#[tokio::test]
async fn sbx_u_014_a_create_that_lands_late_is_removed_and_the_retry_starts_the_sandbox()
-> TestResult {
    let (backend, listener, _storage) =
        retrying(Duration::from_millis(300), Duration::from_secs(5)).await?;
    let cancel = CancellationToken::new();
    let (started, seen) = scripted(
        listener,
        vec![
            // The daemon finishes this create only after the gateway stopped waiting.
            slow(
                step("POST /containers/create", "201 Created", CREATED),
                Duration::from_millis(700),
            ),
            step("GET /containers/tqa-unit-c/json", "404 Not Found", NO_SUCH),
            // By the retry it has landed: removed by name, as this instance's own, then created again.
            step("GET /containers/tqa-unit-c/json", "200 OK", LATE),
            step("DELETE /containers/tqa-unit-c", "204 No Content", ""),
            step("POST /containers/create", "201 Created", CREATED),
            step("POST /containers/c1/start", "204 No Content", ""),
        ],
        Box::pin(backend.start_retried(ContainerCreateBody::default(), "tqa-unit-c", &cancel)),
    )
    .await?;
    assert_eq!(started?, "c1", "{seen:?}");
    assert_eq!(creates(&seen), 2, "{seen:?}");
    Ok(())
}

// TEST: SBX-U-014
#[tokio::test]
async fn sbx_u_014_a_late_create_that_collides_by_name_is_retried_not_left_behind() -> TestResult {
    let (backend, listener, _storage) =
        retrying(Duration::from_millis(300), Duration::from_secs(5)).await?;
    let cancel = CancellationToken::new();
    let conflict =
        r#"{"message":"Conflict. The container name \"/tqa-unit-c\" is already in use"}"#;
    let (started, seen) = scripted(
        listener,
        vec![
            slow(
                step("POST /containers/create", "201 Created", CREATED),
                Duration::from_millis(700),
            ),
            step("GET /containers/tqa-unit-c/json", "404 Not Found", NO_SUCH),
            step("GET /containers/tqa-unit-c/json", "404 Not Found", NO_SUCH),
            // The late create lands between the check and the retry's create: the name is taken.
            step("POST /containers/create", "409 Conflict", conflict),
            step("GET /containers/tqa-unit-c/json", "200 OK", LATE),
            step("DELETE /containers/tqa-unit-c", "204 No Content", ""),
            step("POST /containers/create", "201 Created", CREATED),
            step("POST /containers/c1/start", "204 No Content", ""),
        ],
        Box::pin(backend.start_retried(ContainerCreateBody::default(), "tqa-unit-c", &cancel)),
    )
    .await?;
    assert_eq!(started?, "c1", "{seen:?}");
    assert_eq!(creates(&seen), 3, "{seen:?}");
    Ok(())
}

// TEST: SBX-U-014
#[tokio::test]
async fn sbx_u_014_a_daemon_that_stays_busy_fails_after_its_attempts_with_each_cleaned_up()
-> TestResult {
    let (backend, listener, _storage) =
        retrying(Duration::from_millis(200), Duration::from_secs(8)).await?;
    let cancel = CancellationToken::new();
    let busy = || {
        slow(
            step("POST /containers/create", "201 Created", CREATED),
            Duration::from_millis(400),
        )
    };
    let missing = || step("GET /containers/tqa-unit-c/json", "404 Not Found", NO_SUCH);
    let (started, seen) = scripted(
        listener,
        vec![
            busy(),
            missing(),
            missing(),
            busy(),
            missing(),
            missing(),
            busy(),
            missing(),
        ],
        Box::pin(backend.start_retried(ContainerCreateBody::default(), "tqa-unit-c", &cancel)),
    )
    .await?;
    assert!(
        matches!(started, Err(SandboxError::Timeout)),
        "{started:?} {seen:?}"
    );
    assert_eq!(creates(&seen), 3, "bounded attempts: {seen:?}");
    // Every attempt's create was looked for afterwards, the last one included.
    assert!(
        seen.last()
            .is_some_and(|line| line.contains("/containers/tqa-unit-c/json")),
        "{seen:?}"
    );
    Ok(())
}

// TEST: SBX-U-014
#[tokio::test]
async fn sbx_u_014_no_attempt_starts_once_the_create_budget_is_spent() -> TestResult {
    let (backend, listener, _storage) =
        retrying(Duration::from_millis(300), Duration::from_millis(700)).await?;
    let cancel = CancellationToken::new();
    let started_at = std::time::Instant::now();
    let (started, seen) = scripted(
        listener,
        vec![
            // The daemon answers neither the create nor, until it is done with it, the cleanup's lookup.
            slow(
                step("POST /containers/create", "201 Created", CREATED),
                Duration::from_secs(1),
            ),
            step("GET /containers/tqa-unit-c/json", "404 Not Found", NO_SUCH),
        ],
        Box::pin(backend.start_retried(ContainerCreateBody::default(), "tqa-unit-c", &cancel)),
    )
    .await?;
    assert!(
        matches!(started, Err(SandboxError::Timeout)),
        "{started:?} {seen:?}"
    );
    assert_eq!(
        creates(&seen),
        1,
        "the budget left no time for a second attempt: {seen:?}"
    );
    assert!(
        started_at.elapsed() < Duration::from_secs(3),
        "{:?}",
        started_at.elapsed()
    );
    Ok(())
}

// TEST: SBX-U-015
/// 13h's stack runs under load: attempt 1's create took longer than its bound and was still in flight in the daemon,
/// which held the name; the retry's creates got 409 at once and the start failed with most of its budget unused. A
/// 409 after a timed-out attempt now waits, within the budget, for that create to land, removes it and creates again,
/// and doesn't use up an attempt doing so.
#[tokio::test]
async fn sbx_u_015_a_create_still_in_flight_is_waited_for_then_removed_without_using_up_an_attempt()
-> TestResult {
    let (mut backend, listener, _storage) =
        retrying(Duration::from_millis(300), Duration::from_secs(8)).await?;
    // Two attempts: the conflict must not spend the second one.
    backend.retry = crate::retry::Retry::new(2, Duration::from_millis(1));
    let cancel = CancellationToken::new();
    let conflict =
        r#"{"message":"Conflict. The container name \"/tqa-unit-c\" is already in use"}"#;
    let (started, seen) = scripted(
        listener,
        vec![
            // Attempt 1: the daemon is still creating when the bound ends.
            slow(
                step("POST /containers/create", "201 Created", CREATED),
                Duration::from_millis(700),
            ),
            step("GET /containers/tqa-unit-c/json", "404 Not Found", NO_SUCH),
            // Attempt 2: nothing to remove yet, and the create collides with the one in flight.
            step("GET /containers/tqa-unit-c/json", "404 Not Found", NO_SUCH),
            step("POST /containers/create", "409 Conflict", conflict),
            // Waited for: it lands, is removed as this instance's own, and the create runs again.
            step("GET /containers/tqa-unit-c/json", "404 Not Found", NO_SUCH),
            step("GET /containers/tqa-unit-c/json", "200 OK", LATE),
            step("DELETE /containers/tqa-unit-c", "204 No Content", ""),
            step("POST /containers/create", "201 Created", CREATED),
            step("POST /containers/c1/start", "204 No Content", ""),
        ],
        Box::pin(backend.start_retried(ContainerCreateBody::default(), "tqa-unit-c", &cancel)),
    )
    .await?;
    assert_eq!(started?, "c1", "{seen:?}");
    assert_eq!(creates(&seen), 3, "{seen:?}");
    Ok(())
}

// TEST: SBX-U-015
/// A create that never lands is waited for only within the attempt's bound: the start fails with a timeout inside
/// the budget, as a busy daemon's does.
#[tokio::test]
async fn sbx_u_015_a_conflict_that_never_clears_fails_within_the_budget() -> TestResult {
    let (mut backend, listener, _storage) =
        retrying(Duration::from_millis(300), Duration::from_millis(1500)).await?;
    backend.retry = crate::retry::Retry::new(2, Duration::from_millis(1));
    let cancel = CancellationToken::new();
    let conflict =
        r#"{"message":"Conflict. The container name \"/tqa-unit-c\" is already in use"}"#;
    let missing = || step("GET /containers/tqa-unit-c/json", "404 Not Found", NO_SUCH);
    let started_at = std::time::Instant::now();
    let steps = vec![
        slow(
            step("POST /containers/create", "201 Created", CREATED),
            Duration::from_millis(700),
        ),
        missing(),
        missing(),
        step("POST /containers/create", "409 Conflict", conflict),
        // The daemon still has the name and doesn't answer the look-up within what is left of the bound.
        slow(missing(), Duration::from_secs(1)),
    ];
    let (started, seen) = scripted(
        listener,
        steps,
        Box::pin(backend.start_retried(ContainerCreateBody::default(), "tqa-unit-c", &cancel)),
    )
    .await?;
    assert!(
        matches!(started, Err(SandboxError::Timeout)),
        "{started:?} {seen:?}"
    );
    assert!(
        started_at.elapsed() < Duration::from_millis(2500),
        "{:?}",
        started_at.elapsed()
    );
    Ok(())
}
