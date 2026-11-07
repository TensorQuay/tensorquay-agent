//! GW-I-004: only accepted messages and newly stored terminal run events advance activity.

#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, run};
use tqa_gateway::Conversation;
use tqa_types::{Event, ServerFrame};

async fn prime(fixture: &Fixture, conversation: &Conversation) -> TestResult {
    sqlx::query("UPDATE conversations SET updated_at='2001-01-01Z',last_activity_at='2000-01-01Z' WHERE id=$1")
        .bind(uuid::Uuid::parse_str(conversation.id.as_str())?)
        .execute(&fixture.pool).await?;
    Ok(())
}

async fn metadata(
    fixture: &Fixture,
    conversation: &Conversation,
    cookie: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let response = fixture
        .get(
            &format!("/api/v1/conversations/{}", conversation.id),
            cookie,
        )
        .await?;
    assert_eq!(response.status(), 200);
    Ok(response.json().await?)
}

fn terminal(event: Event) -> ServerFrame {
    match support::frame(1, "unused") {
        ServerFrame::Event {
            seq, run_id, step, ..
        } => ServerFrame::Event {
            seq,
            run_id,
            step,
            ts: "2100-01-01T00:00:00Z".into(),
            event,
        },
        _ => unreachable!("fixture produces an event"),
    }
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_004_accepted_message_advances_activity_in_its_transaction(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (user, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(user.id).await?;
        prime(fixture, &conversation).await?;
        let response = fixture
            .action(
                &format!("/api/v1/conversations/{}/messages", conversation.id),
                &cookie,
                Some("activity-message"),
                &json!({"text":"inspect"}),
            )
            .await?;
        assert_eq!(response.status(), 202);
        let result = metadata(fixture, &conversation, &cookie).await?;
        assert_ne!(result["last_activity_at"], "2000-01-01T00:00:00Z");
        assert_eq!(result["updated_at"], "2001-01-01T00:00:00Z");
        let list: Value = fixture
            .get("/api/v1/conversations", &cookie)
            .await?
            .json()
            .await?;
        assert_eq!(
            list["items"][0]["last_activity_at"],
            result["last_activity_at"]
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_004_text_delta_does_not_advance_activity(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let (user, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(user.id).await?;
        prime(fixture, &conversation).await?;
        fixture
            .gateway
            .events()
            .append(&conversation.id, support::frame(1, "fragment"))
            .await?;
        assert_eq!(
            metadata(fixture, &conversation, &cookie).await?["last_activity_at"],
            "2000-01-01T00:00:00Z"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_004_terminal_events_use_database_time_and_preserve_metadata_time(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (user, cookie) = fixture.admin().await?;
        for event in [
            Event::RunCompleted {
                run_id: "run-example".parse()?,
            },
            Event::RunFailed {
                run_id: "run-example".parse()?,
                reason: "test failure".into(),
                code: tqa_types::ErrorCode::SandboxFailed,
                provider_message: None,
            },
            Event::RunCancelled {
                run_id: "run-example".parse()?,
            },
        ] {
            let conversation = fixture.conversation(user.id).await?;
            prime(fixture, &conversation).await?;
            let before: time::OffsetDateTime = sqlx::query_scalar("SELECT CURRENT_TIMESTAMP")
                .fetch_one(&fixture.pool)
                .await?;
            fixture
                .gateway
                .events()
                .append(&conversation.id, terminal(event))
                .await?;
            let after: time::OffsetDateTime = sqlx::query_scalar("SELECT CURRENT_TIMESTAMP")
                .fetch_one(&fixture.pool)
                .await?;
            let result = metadata(fixture, &conversation, &cookie).await?;
            let value = time::OffsetDateTime::parse(
                result["last_activity_at"]
                    .as_str()
                    .ok_or("missing activity")?,
                &time::format_description::well_known::Rfc3339,
            )?;
            assert!((before..=after).contains(&value));
            assert_eq!(result["updated_at"], "2001-01-01T00:00:00Z");
        }
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_004_duplicate_terminal_event_does_not_advance_activity(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let (user, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(user.id).await?;
        let frame = terminal(Event::RunCompleted {
            run_id: "run-example".parse()?,
        });
        fixture
            .gateway
            .events()
            .append(&conversation.id, frame.clone())
            .await?;
        prime(fixture, &conversation).await?;
        assert!(
            fixture
                .gateway
                .events()
                .append(&conversation.id, frame)
                .await
                .is_err()
        );
        assert_eq!(
            metadata(fixture, &conversation, &cookie).await?["last_activity_at"],
            "2000-01-01T00:00:00Z"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_004_rejected_or_replayed_action_does_not_advance_activity(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (user, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(user.id).await?;
        let path = format!("/api/v1/conversations/{}/messages", conversation.id);
        assert_eq!(
            fixture
                .action(
                    &path,
                    &cookie,
                    Some("unchanged-activity"),
                    &json!({"text":"first"})
                )
                .await?
                .status(),
            202
        );
        prime(fixture, &conversation).await?;
        for (text, status) in [("first", 202), ("different", 422)] {
            assert_eq!(
                fixture
                    .action(
                        &path,
                        &cookie,
                        Some("unchanged-activity"),
                        &json!({"text":text})
                    )
                    .await?
                    .status(),
                status
            );
            assert_eq!(
                metadata(fixture, &conversation, &cookie).await?["last_activity_at"],
                "2000-01-01T00:00:00Z"
            );
        }
        Ok(())
    })
    .await
}
