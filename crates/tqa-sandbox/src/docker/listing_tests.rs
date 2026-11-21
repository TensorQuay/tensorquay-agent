//! SBX-U-022 (18.1.1): one container carrying this instance's labels that can't be read (a label malformed) never fails
//! the listing: the readable ones are listed, and the unreadable one is left out and noted for the sweep to count.
use super::sandbox_network_tests::{TestResult, scripted, scripted_backend, step};
use crate::SandboxBackend;

/// One readable sandbox and two that aren't: a creation time that isn't a number, and a task id that isn't one.
const CONTAINERS: &str = r#"[
    {"Id":"good1","Labels":{"tqa.instance":"tqa-unit","tqa.conversation":"conversation_a","tqa.created_at":"1"}},
    {"Id":"badtime","Created":1700000000,"Labels":{"tqa.instance":"tqa-unit","tqa.conversation":"conversation_b","tqa.created_at":"soon"}},
    {"Id":"badtask","Labels":{"tqa.instance":"tqa-unit","tqa.conversation":"not a task id!","tqa.created_at":"2"}}
]"#;

// TEST: SBX-U-022
#[tokio::test]
async fn sbx_u_022_an_unreadable_container_is_left_out_of_the_listing_and_noted() -> TestResult {
    let (backend, listener, _storage) = scripted_backend(None).await?;
    let (listed, _) = scripted(
        listener,
        vec![step("GET /containers/json", "200 OK", CONTAINERS)],
        Box::pin(backend.listing()),
    )
    .await?;
    let listed = listed?;
    assert_eq!(
        listed
            .handles
            .iter()
            .map(|handle| handle.id.as_str())
            .collect::<Vec<_>>(),
        vec!["good1"]
    );
    assert_eq!(
        listed
            .unreadable
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["badtime", "badtask"]
    );
    Ok(())
}

// TEST: SBX-U-025
/// An unreadable container keeps the engine's own creation time from the listing (none when the engine gives none), so
/// the sweep lists its true age rather than 0 s (decided on 21 Nov 2026).
#[tokio::test]
async fn sbx_u_025_an_unreadable_container_keeps_the_engines_creation_time() -> TestResult {
    let (backend, listener, _storage) = scripted_backend(None).await?;
    let (listed, _) = scripted(
        listener,
        vec![step("GET /containers/json", "200 OK", CONTAINERS)],
        Box::pin(backend.listing()),
    )
    .await?;
    let listed = listed?;
    let created = |id: &str| {
        listed
            .unreadable
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.created_at)
    };
    assert_eq!(
        created("badtime"),
        Some(Some(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)
        ))
    );
    assert_eq!(created("badtask"), Some(None));
    Ok(())
}

// TEST: SBX-U-023
/// Each listing answers its own unreadable containers with its handles, as a value: the first listing's bad containers
/// are its own, and a second listing without them leaves the first's answer as it was.
#[tokio::test]
async fn sbx_u_023_each_listing_answers_its_own_unreadable_containers() -> TestResult {
    let (backend, listener, _storage) = scripted_backend(None).await?;
    let only_good = r#"[{"Id":"good1","Labels":{"tqa.instance":"tqa-unit","tqa.conversation":"conversation_a","tqa.created_at":"1"}}]"#;
    let ((first, second), _) = scripted(
        listener,
        vec![
            step("GET /containers/json", "200 OK", CONTAINERS),
            step("GET /containers/json", "200 OK", only_good),
        ],
        Box::pin(async {
            let first = backend.listing().await;
            let second = backend.listing().await;
            (first, second)
        }),
    )
    .await?;
    let (first, second) = (first?, second?);
    assert_eq!(
        first
            .unreadable
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["badtime", "badtask"]
    );
    assert_eq!(first.handles.len(), 1);
    assert_eq!((second.unreadable.len(), second.handles.len()), (0, 1));
    Ok(())
}
