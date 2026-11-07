//! GW-I-072 (ADR-0031, G6): a conversation starts with the enabled connectors marked `enabled_by_default`, changes its
//! set by PATCH within the existing connectors and the runtime's bound, and each session gets its catalogue servers and
//! connectors as `session.config.mcp` entries; a disabled connector is left out of the next session at once.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::fmt::Write as _;
use support::{Fixture, TestResult, call};
use tqa_types::ConversationId;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const CONFIG: &str = "[[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\nenabled_by_default=true\n\
    [[connectors]]\nname='tracker'\nurl='https://mcp.tracker.example/mcp'\nenabled_by_default=true\n\
    [[connectors]]\nname='wiki'\nurl='https://mcp.wiki.example/mcp'\n\
    [[connectors]]\nname='off'\nurl='https://mcp.off.example/mcp'\nenabled=false\nenabled_by_default=true\n";

async fn patch(
    f: &Fixture,
    id: &ConversationId,
    cookie: &str,
    key: &str,
    body: &Value,
) -> Result<(u16, Value)> {
    let response = f
        .client
        .patch(f.url(&format!("/api/v1/conversations/{id}")))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .header("idempotency-key", key)
        .json(body)
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await.unwrap_or(Value::Null)))
}

async fn stored(f: &Fixture, id: &ConversationId, cookie: &str) -> Result<Value> {
    Ok(f.client
        .get(f.url(&format!("/api/v1/conversations/{id}")))
        .header("cookie", cookie)
        .send()
        .await?
        .json()
        .await?)
}

/// The `mcp` entries of a fresh session of the conversation.
async fn session_mcp(f: &Fixture, id: &ConversationId) -> Result<Value> {
    let runtime = f.runtime_for(id).await?;
    let mcp = runtime.config["mcp"].clone();
    drop(runtime);
    Ok(mcp)
}

fn names(entries: &Value) -> Vec<&str> {
    entries
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry["name"].as_str())
                .collect()
        })
        .unwrap_or_default()
}

// TEST: GW-I-072
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_072_connectors_are_chosen_per_conversation_and_delivered_to_each_session(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = conversation.id.clone();
        // The defaults: enabled and marked; `off` is marked but disabled, `wiki` is not marked.
        assert_eq!(
            stored(f, &id, &cookie).await?["connectors"],
            json!(["tracker"])
        );
        let mcp = session_mcp(f, &id).await?;
        assert_eq!(names(&mcp), ["docs", "tracker"]);
        assert_eq!(mcp[0]["transport"]["kind"], "stdio");
        assert_eq!(
            (&mcp[1]["transport"], mcp[1].get("class")),
            (&json!({"kind": "gateway", "connector": "tracker"}), None),
            "no URL: the runtime derives it from its gateway origin"
        );
        // PATCH within the existing connectors.
        let (status, body) = patch(
            f,
            &id,
            &cookie,
            "both",
            &json!({"connectors": ["tracker", "wiki"]}),
        )
        .await?;
        assert_eq!(
            (status, &body["connectors"]),
            (200, &json!(["tracker", "wiki"]))
        );
        for (key, refused) in [
            ("unknown", json!({"connectors": ["nowhere"]})),
            ("twice", json!({"connectors": ["wiki", "wiki"]})),
            ("typed", json!({"connectors": "wiki"})),
        ] {
            assert_eq!(patch(f, &id, &cookie, key, &refused).await?.0, 400, "{key}");
        }
        assert_eq!(
            names(&session_mcp(f, &id).await?),
            ["docs", "tracker", "wiki"]
        );
        // Disabling a connector leaves it out of the next session at once; the conversation keeps it in its set.
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/admin/connectors/wiki",
            &cookie,
            (
                Some(
                    json!({"url": "https://mcp.wiki.example/mcp", "enabled": false, "version": 1}),
                ),
                None,
            ),
        )
        .await?;
        assert_eq!(status, 200);
        assert_eq!(names(&session_mcp(f, &id).await?), ["docs", "tracker"]);
        assert_eq!(
            stored(f, &id, &cookie).await?["connectors"],
            json!(["tracker", "wiki"])
        );
        // An empty set delivers the catalogue servers only.
        assert_eq!(
            patch(f, &id, &cookie, "none", &json!({"connectors": []}))
                .await?
                .0,
            200
        );
        assert_eq!(names(&session_mcp(f, &id).await?), ["docs"]);
        Ok(())
    })
    .await
}

// TEST: GW-I-072
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_072_servers_and_connectors_share_the_runtimes_bound(pool: PgPool) -> TestResult {
    let connectors = (0..8).fold(String::new(), |mut text, index| {
        let _ = write!(
            text,
            "[[connectors]]\nname='c{index}'\nurl='https://mcp.example/{index}'\nenabled_by_default=true\n"
        );
        text
    });
    let config = format!(
        "[[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\nenabled_by_default=true\n{connectors}"
    );
    support::run_configured(pool, None, &config, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = conversation.id.clone();
        // One server leaves room for seven connectors, in name order.
        let chosen = stored(f, &id, &cookie).await?["connectors"].clone();
        assert_eq!(chosen, json!(["c0", "c1", "c2", "c3", "c4", "c5", "c6"]));
        let all: Vec<String> = (0..8).map(|index| format!("c{index}")).collect();
        let (status, _) = patch(f, &id, &cookie, "all", &json!({"connectors": all})).await?;
        assert_eq!(status, 400, "nine entries in all");
        assert_eq!(
            stored(f, &id, &cookie).await?["connectors"],
            chosen,
            "a refused change leaves the set"
        );
        let (status, _) = patch(
            f,
            &id,
            &cookie,
            "swap",
            &json!({"mcp_servers": [], "connectors": all}),
        )
        .await?;
        assert_eq!(status, 200, "eight connectors without the server");
        assert_eq!(names(&session_mcp(f, &id).await?).len(), 8);
        Ok(())
    })
    .await
}
