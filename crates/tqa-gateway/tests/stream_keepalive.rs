//! GW-I-201: a task's stream keeps its connection alive while a run is active and silent (a long tool call with no
//! output): it sends a keep-alive comment every `stream_keepalive`, so nothing between the gateway and the browser
//! closes it as idle and the page never shows "Reconnecting…" for a quiet run. The browser's own side (an
//! `EventSource` reconnects only on an error) is the web's test.
#[path = "support/fixture.rs"]
mod support;

use futures::StreamExt;
use sqlx::PgPool;
use std::time::Duration;
use support::TestResult;
use tqa_types::{Event, ServerFrame};

// TEST: GW-I-201
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_201_a_silent_active_run_keeps_its_stream_alive(pool: PgPool) -> TestResult {
    // The fixture's `stream_keepalive` is one second, so 3.5 s of silence must carry at least two.
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = conversation.id.clone();
        let mut runtime = f.runtime(&conversation).await?;
        runtime
            .send(&ServerFrame::Event {
                seq: 1.into(),
                run_id: Some("r-1".parse()?),
                step: None,
                ts: "2026-01-01T00:00:00Z".into(),
                event: Event::RunStarted {
                    run_id: "r-1".parse()?,
                },
            })
            .await?;
        f.until_sequence(&id, 1).await?;
        let response = support::stream_client()?
            .get(f.url(&format!(
                "/api/v1/conversations/{id}/events?stream=1&protocol=1"
            )))
            .header("cookie", &cookie)
            .header("last-event-id", "1")
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        let mut chunks = response.bytes_stream();
        let mut text = String::new();
        // The run goes on with nothing to say, as during a long tool call: only keep-alives arrive.
        let quiet = tokio::time::Instant::now() + Duration::from_millis(3500);
        while let Ok(Some(chunk)) = tokio::time::timeout_at(quiet, chunks.next()).await {
            text.push_str(&String::from_utf8_lossy(&chunk?));
        }
        let kept = text.matches(": keep-alive").count();
        assert!(
            kept >= 2,
            "at least two keep-alives in 3.5 s of silence, got {kept}: {text:?}"
        );
        assert!(
            !text.contains("run.completed") && !text.contains("run.failed"),
            "the run is still going"
        );
        drop(runtime);
        Ok(())
    })
    .await
}
