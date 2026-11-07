//! A frame whose strings or keys contain NUL (U+0000) is stored and replayed unchanged (task 056).
#[path = "support/fixture.rs"]
mod support;

use secrecy::ExposeSecret;
use serde_json::json;
use sqlx::PgPool;
use support::TestResult;
use tqa_types::{ClientFrame, ErrorCode, Event, SandboxState, ServerFrame};

fn tool_call(seq: u64) -> Result<ServerFrame, Box<dyn std::error::Error>> {
    Ok(ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: Event::ToolCall {
            call_id: "call-1".parse()?,
            name: "shell".into(),
            arguments: json!({"command":"printf 'a\u{0}b'","k\u{0}ey":["\u{0}"]}),
        },
    })
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn a_frame_with_nul_is_stored_streamed_and_replayed_unchanged(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let mut live = f.stream(id, &cookie, 0).await?;
        let sent = tool_call(1)?;
        runtime.send(&sent).await?;
        // No error comes back on the link (a failed store answers `store_unavailable` here).
        runtime.roundtrip().await?;
        // The run continues: the next frame is accepted after the NUL frame.
        runtime.send(&support::frame(2, "after")).await?;
        f.until_sequence(id, 2).await?;
        assert_eq!(live.durable().await?, (1.into(), sent.clone()), "live");
        assert_eq!(live.durable().await?.0, 2.into());
        let mut replay = f.stream(id, &cookie, 0).await?;
        assert_eq!(
            replay.durable().await?,
            (1.into(), sent.clone()),
            "SSE replay"
        );
        let token = f.backend.token(id).ok_or("runtime token")?;
        let page: serde_json::Value = f
            .client
            .get(f.url("/runtime/history?after=0&limit=2"))
            .bearer_auth(token.expose_secret())
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(
            page["items"][0],
            serde_json::to_value(&sent)?,
            "link history"
        );
        assert_eq!(page["items"][1]["seq"], 2);
        Ok(())
    })
    .await
}

fn event(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn a_user_message_with_nul_is_queued_delivered_and_replayed_unchanged(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let body = json!({"text":"before\u{0}after"});
        for _ in 0..2 {
            let accepted = f
                .action(
                    &format!("/api/v1/conversations/{id}/messages"),
                    &cookie,
                    Some("nul"),
                    &body,
                )
                .await?;
            assert_eq!(accepted.status(), 202, "accepted, and the same key replays");
        }
        match runtime.action().await? {
            ClientFrame::UserMessage { text, .. } => assert_eq!(text, "before\u{0}after"),
            other => return Err(format!("expected the user message, received {other:?}").into()),
        }
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn a_run_failure_with_nul_in_its_reason_ends_the_run(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        runtime
            .send(&event(
                1,
                Event::RunStarted {
                    run_id: "r-1".parse()?,
                },
            ))
            .await?;
        f.until_sequence(id, 1).await?;
        assert_eq!(f.stream(id, &cookie, 1).await?.initial, SandboxState::Busy);
        runtime
            .send(&event(
                2,
                Event::RunFailed {
                    run_id: "r-1".parse()?,
                    reason: "model said \u{0}".into(),
                    code: ErrorCode::SandboxFailed,
                    provider_message: None,
                },
            ))
            .await?;
        f.until_sequence(id, 2).await?;
        // Run activity must see the terminal event even though its frame cannot live in `jsonb`.
        assert_eq!(f.stream(id, &cookie, 2).await?.initial, SandboxState::Idle);
        Ok(())
    })
    .await
}
