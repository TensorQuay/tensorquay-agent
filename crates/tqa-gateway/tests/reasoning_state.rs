//! GW-I-051 / SEC-021: clients never receive opaque reasoning items; the runtime's history returns them
//! unchanged.
#[path = "support/fixture.rs"]
mod support;

use secrecy::ExposeSecret;
use serde_json::json;
use sqlx::PgPool;
use support::TestResult;
use tqa_types::ServerFrame;

const SECRET: &str = "opaque-encrypted-marker";

fn state(seq: u64) -> Result<ServerFrame, Box<dyn std::error::Error>> {
    let item =
        json!({"type":"reasoning","id":"rs_1","encrypted_content":SECRET,"summary":[]}).to_string();
    frame(seq, "openai_responses", &item)
}

fn frame(seq: u64, protocol: &str, item: &str) -> Result<ServerFrame, Box<dyn std::error::Error>> {
    let frame = json!({"type":"event","seq":seq,"run_id":null,"step":null,"ts":"2026-01-01T00:00:00Z",
        "event":{"type":"reasoning.state","protocol":protocol,"route":"local","model":"m",
            "bytes":item.len(),"item":item}});
    Ok(ServerFrame::parse(&serde_json::to_vec(&frame)?)?)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_051_clients_see_the_state_without_its_item_and_the_runtime_gets_it_whole(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let mut live = f.stream(id, &cookie, 0).await?;
        let sent = state(1)?;
        let client = sent.clone().for_clients();
        runtime.send(&sent).await?;
        runtime.roundtrip().await?;
        f.until_sequence(id, 1).await?;
        assert_eq!(
            live.durable().await?,
            (1.into(), client.clone()),
            "live SSE"
        );
        let mut replay = f.stream(id, &cookie, 0).await?;
        assert_eq!(
            replay.durable().await?,
            (1.into(), client.clone()),
            "SSE replay"
        );
        let page = f
            .get(
                &format!("/api/v1/conversations/{id}/events?after=0&limit=10"),
                &cookie,
            )
            .await?
            .text()
            .await?;
        assert!(!page.contains(SECRET), "events API: {page}");
        let page: serde_json::Value = serde_json::from_str(&page)?;
        assert_eq!(
            page["items"][0],
            serde_json::to_value(&client)?,
            "events API"
        );
        assert!(
            page["items"][0]["event"]["bytes"]
                .as_u64()
                .is_some_and(|bytes| bytes > 0)
        );
        let token = f.backend.token(id).ok_or("runtime token")?;
        let history: serde_json::Value = f
            .client
            .get(f.url("/runtime/history?after=0&limit=10"))
            .bearer_auth(token.expose_secret())
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(
            history["items"][0],
            serde_json::to_value(&sent)?,
            "runtime history"
        );
        Ok(())
    })
    .await
}

/// ADR-0027 amendment (Chat tool-call extras): today's gateway already stores a Chat-protocol state and returns it whole
/// to the runtime, without it reaching clients, so a runtime that starts sending them needs no protocol revision.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_051_a_chat_tool_call_extra_is_stored_and_returned_like_any_state(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let item = json!({"type":"chat.tool_call_extra","call_id":"call_1",
            "extra_content":{"google":{"thought_signature":SECRET}}})
        .to_string();
        let sent = frame(1, "openai_chat", &item)?;
        runtime.send(&sent).await?;
        runtime.roundtrip().await?;
        f.until_sequence(id, 1).await?;
        let page = f
            .get(
                &format!("/api/v1/conversations/{id}/events?after=0&limit=10"),
                &cookie,
            )
            .await?
            .text()
            .await?;
        assert!(!page.contains(SECRET), "events API: {page}");
        let token = f.backend.token(id).ok_or("runtime token")?;
        let history: serde_json::Value = f
            .client
            .get(f.url("/runtime/history?after=0&limit=10"))
            .bearer_auth(token.expose_secret())
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(
            history["items"][0],
            serde_json::to_value(&sent)?,
            "runtime history"
        );
        Ok(())
    })
    .await
}
