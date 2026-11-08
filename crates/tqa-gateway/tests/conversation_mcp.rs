//! GW-I-067 (ADR-0011): a conversation starts with the catalogue's enabled-by-default MCP servers,
//! and `PATCH /conversations/{id}` changes its set within the catalogue, never during a run.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, Event, ServerFrame};

const CATALOGUE: &str = "[[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\nenabled_by_default=true\n\
    [[mcp.servers]]\nname='lint'\ncommand='/usr/local/bin/lint-mcp'\n";

async fn patch(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
    key: &str,
    body: &Value,
) -> Result<reqwest::Response, reqwest::Error> {
    f.client
        .patch(f.url(&format!("/api/v1/conversations/{id}")))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .header("idempotency-key", key)
        .json(body)
        .send()
        .await
}

async fn servers(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let stored: Value = f
        .client
        .get(f.url(&format!("/api/v1/conversations/{id}")))
        .header("cookie", cookie)
        .send()
        .await?
        .json()
        .await?;
    Ok(stored["mcp_servers"].clone())
}

// TEST: GW-I-067
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_067_a_conversation_starts_with_the_default_servers_and_changes_them_within_the_catalogue(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CATALOGUE, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        assert_eq!(
            servers(f, id, &cookie).await?,
            json!(["docs"]),
            "the enabled-by-default set"
        );
        let changed = patch(
            f,
            id,
            &cookie,
            "both",
            &json!({"mcp_servers":["lint","docs"]}),
        )
        .await?;
        assert_eq!(changed.status(), 200);
        assert_eq!(
            changed.json::<Value>().await?["mcp_servers"],
            json!(["lint", "docs"])
        );
        let none = patch(f, id, &cookie, "none", &json!({"mcp_servers":[]})).await?;
        assert_eq!(none.status(), 200);
        assert_eq!(
            servers(f, id, &cookie).await?,
            json!([]),
            "every server switched off"
        );
        for (key, body) in [
            ("unknown", json!({"mcp_servers":["docs","absent"]})),
            ("repeated", json!({"mcp_servers":["docs","docs"]})),
            ("typed", json!({"mcp_servers":"docs"})),
        ] {
            let refused = patch(f, id, &cookie, key, &body).await?;
            assert_eq!(refused.status(), 400, "{key}");
        }
        assert_eq!(
            servers(f, id, &cookie).await?,
            json!([]),
            "a refused change changes nothing"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-067
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_067_the_server_set_cannot_change_during_a_run(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, CATALOGUE, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        runtime
            .send(&ServerFrame::Event {
                seq: 1.into(),
                run_id: None,
                step: None,
                ts: "2026-01-01T00:00:00Z".into(),
                event: Event::RunStarted {
                    run_id: "active".parse()?,
                },
            })
            .await?;
        f.until_sequence(id, 1).await?;
        let during = patch(f, id, &cookie, "during", &json!({"mcp_servers":["lint"]})).await?;
        assert_eq!(during.status(), 409);
        assert_eq!(during.json::<Value>().await?["error"]["code"], "run_active");
        assert_eq!(servers(f, id, &cookie).await?, json!(["docs"]));
        // Another user cannot change it.
        let (_, other) = f.member("other@example.invalid").await?;
        let foreign = patch(f, id, &other, "foreign", &json!({"mcp_servers":[]})).await?;
        assert_eq!(foreign.status(), 403);
        Ok(())
    })
    .await
}

// TEST: GW-I-067
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_067_without_a_catalogue_a_conversation_has_no_servers(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        assert_eq!(servers(f, &conversation.id, &cookie).await?, json!([]));
        let refused = patch(
            f,
            &conversation.id,
            &cookie,
            "any",
            &json!({"mcp_servers":["docs"]}),
        )
        .await?;
        assert_eq!(refused.status(), 400, "no catalogue, nothing to choose");
        Ok(())
    })
    .await
}
