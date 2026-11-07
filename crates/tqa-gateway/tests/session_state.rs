//! GW-I-017/WEB-E2E-004: an unallocated conversation must permit its first message.

#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use support::TestResult;
use tqa_types::{ConversationId, SandboxState, ServerFrame};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_017_runtime_registration_updates_an_already_open_client_stream(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let mut stream = fixture.stream(&conversation.id, &cookie, 0).await?;
        let _runtime = fixture.runtime(&conversation).await?;
        let (sequence, frame) = stream.next().await?;
        assert_eq!(sequence, None);
        assert_eq!(
            frame,
            ServerFrame::SessionReady {
                conversation_id: conversation.id.clone(),
                resumed: false,
                sandbox: tqa_types::SandboxSummary::new(conversation.backend, SandboxState::Ready),
                queue: None,
            }
        );
        assert!(
            fixture
                .gateway
                .events()
                .read(&conversation.id, 0.into(), 10)
                .await?
                .is_empty()
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_017_session_distinguishes_unallocated_starting_and_registered_sandboxes(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let id = &conversation.id;
        assert_eq!(
            initial_state(fixture, id, &cookie).await?,
            SandboxState::Destroyed
        );
        assert_eq!(
            fixture
                .backend
                .creates
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        fixture.gateway.sandboxes().ensure(id).await?;
        assert_eq!(
            initial_state(fixture, id, &cookie).await?,
            SandboxState::Creating
        );
        let _runtime = fixture.runtime(&conversation).await?;
        assert_eq!(
            initial_state(fixture, id, &cookie).await?,
            SandboxState::Ready
        );
        Ok(())
    })
    .await
}

async fn initial_state(
    fixture: &support::Fixture,
    id: &ConversationId,
    cookie: &str,
) -> Result<SandboxState, Box<dyn std::error::Error>> {
    let mut response = fixture
        .get(
            &format!("/api/v1/conversations/{id}/events?stream=1&protocol=1"),
            cookie,
        )
        .await?;
    assert_eq!(response.status(), 200);
    let mut bytes = Vec::new();
    while !bytes.windows(2).any(|pair| pair == b"\n\n") {
        bytes.extend_from_slice(&response.chunk().await?.ok_or("session stream closed")?);
        assert!(bytes.len() <= tqa_types::MAX_FRAME_BYTES);
    }
    let text = std::str::from_utf8(&bytes)?;
    let data = text
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .ok_or("missing session frame")?;
    match serde_json::from_str::<ServerFrame>(data.trim())? {
        ServerFrame::SessionReady {
            conversation_id,
            sandbox,
            ..
        } if conversation_id == *id => Ok(sandbox.state),
        _ => Err("unexpected initial session frame".into()),
    }
}
