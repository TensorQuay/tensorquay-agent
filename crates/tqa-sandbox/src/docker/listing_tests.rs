//! SBX-U-022 (18.1.1): one container carrying this instance's labels that can't be read (a label malformed) never fails
//! the listing: the readable ones are listed, and the unreadable one is left out and noted for the sweep to count.
use super::sandbox_network_tests::{TestResult, scripted, scripted_backend, step};
use crate::SandboxBackend;

/// One readable sandbox and two that aren't: a creation time that isn't a number, and a task id that isn't one.
const CONTAINERS: &str = r#"[
    {"Id":"good1","Labels":{"tqa.instance":"tqa-unit","tqa.conversation":"conversation_a","tqa.created_at":"1"}},
    {"Id":"badtime","Labels":{"tqa.instance":"tqa-unit","tqa.conversation":"conversation_b","tqa.created_at":"soon"}},
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
        listed.unreadable,
        vec!["badtime".to_owned(), "badtask".to_owned()]
    );
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
        first.unreadable,
        vec!["badtime".to_owned(), "badtask".to_owned()]
    );
    assert_eq!(first.handles.len(), 1);
    assert_eq!((second.unreadable.len(), second.handles.len()), (0, 1));
    Ok(())
}
