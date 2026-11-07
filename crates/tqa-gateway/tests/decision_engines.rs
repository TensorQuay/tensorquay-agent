//! GW-I-105 (the 11a parts, ADR-0013 point 6): a decision engine seeded from `[[decision_engines]]` in gateway.toml, and
//! the relay `/relay/d/<engine>/v1/<point>`. The relay forwards with the engine's credential from the secret store (the
//! runtime never holds it); only to the engine the conversation's profile names; never with history the engine's
//! exposure excludes, since a tool could read the runtime token and call here directly; within 64 KiB both ways and
//! the engine's deadline; recording a usage row `decision:<name>` and an audited secret read. A message to a
//! conversation whose profile names an engine the install no longer has is refused, never run without it.
#[path = "support/fixture.rs"]
mod support;

use secrecy::SecretString;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{net::Ipv4Addr, time::Duration};
use support::TestResult;
use tqa_testkit::{EngineAnswer, FakeEngineServer};
use tqa_types::decision_v1::Response;

fn config(engine: &FakeEngineServer) -> String {
    format!(
        "[[decision_engines]]\nname = \"router\"\nurl = \"{}\"\ninsecure_http = true\nauth = {{ scheme = \"bearer\" }}\nkey_ref = \"secrets/engine\"\ntimeout_ms = 300\nexposure = \"none\"\n",
        engine.base_url()
    )
}
fn request(point: &str) -> Value {
    json!({
        "point": point,
        "run_id": "r-1",
        "step": 1,
        "attempt": 1,
        "routes": [{"name":"local","context_window":null,"max_output":null,"native_tools":true}],
        "tools": [{"name":"read_file","description":"Read","parameters":{"type":"object"}}],
        "budget": {"steps_used":0,"max_steps":30,"output_tokens_used":0,"max_output_tokens":1000},
        "previous_calls": []
    })
}

/// A conversation of `admin` whose profile names `engine`, a runtime token for it, and the engine's key in the store.
async fn caller(
    f: &support::Fixture,
    admin: &(tqa_gateway::User, String),
    engine: Option<&str>,
) -> Result<(tqa_gateway::Conversation, SecretString, String), Box<dyn std::error::Error>> {
    let (owner, cookie) = (admin.0.clone(), admin.1.clone());
    let conversation = f.conversation(owner.id).await?;
    let profile = json!({"name":"p","display_name":"P","skills":"all","mcp_servers":[],"connectors":[],"rules":[],
        "limits":{},"decision_engine":engine});
    // A conversation made from a profile keeps its copy with the profile's id and version.
    sqlx::query(
        "UPDATE conversations SET profile=$2, profile_id=$3, profile_version=1 WHERE id=$1::uuid",
    )
    .bind(conversation.id.as_str())
    .bind(&profile)
    .bind(uuid::Uuid::now_v7())
    .execute(&f.pool)
    .await?;
    let token = f
        .gateway
        .runtime_tokens()
        .mint(
            &conversation.id,
            &format!("engine-{}", uuid::Uuid::now_v7().simple()).parse()?,
            Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    f.gateway
        .secret_store()
        .put(&"secrets/engine".parse()?, SecretString::from("engine-key"))
        .await?;
    Ok((conversation, token, cookie))
}
async fn consult(
    f: &support::Fixture,
    token: &SecretString,
    path: &str,
    body: &Value,
) -> Result<(u16, String), Box<dyn std::error::Error>> {
    use secrecy::ExposeSecret;
    let response = f
        .client
        .post(f.url(path))
        .bearer_auth(token.expose_secret())
        .json(body)
        .send()
        .await?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let text = response.text().await?;
    support::gateway_refusal(
        &headers,
        &serde_json::from_str(&text).unwrap_or(Value::Null),
    )?;
    Ok((status, text))
}

// TEST: GW-I-105
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_105_the_relay_forwards_with_the_engines_credential_and_records_usage(
    pool: PgPool,
) -> TestResult {
    let engine = FakeEngineServer::start(vec![EngineAnswer::Decide(Response::CallModel {
        route: "local".into(),
        tools: vec!["read_file".into()],
    })])
    .await?;
    support::run_configured(pool, None, &config(&engine), async |f| {
        let admin = f.admin().await?;
        let (conversation, token, _) = caller(f, &admin, Some("router")).await?;
        // The run the question names is the task's own, so its usage row and key read record it (GW-I-151).
        f.gateway.events().append(&conversation.id, support::started(1, "r-1")?).await?;
        let (status, body) = consult(f, &token, "/relay/d/router/v1/before_step", &request("before_step")).await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            serde_json::from_str::<Value>(&body)?,
            json!({"decision":"call_model","route":"local","tools":["read_file"]})
        );
        let seen = engine.requests();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].authorization.as_deref(), Some("Bearer engine-key"), "the engine's key, not the runtime's");
        let usage: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM usage WHERE conversation_id=$1::uuid AND route='decision:router' AND run_id='r-1'",
        )
        .bind(conversation.id.as_str())
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(usage, 1, "a usage row per consultation");
        // The read audit is written off the read's path by one batching writer (ADR-0033's read audit, best-effort);
        // flush it before counting, as GW-I-094 does, so the row is there however loaded the host is.
        f.gateway.read_audit_flush_now().await?;
        let reads: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM secret_reads WHERE consumer_kind='engine' AND consumer='router' AND outcome='ok'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(reads, 1, "the engine's key read is audited");
        Ok(())
    })
    .await?;
    engine.stop().await;
    Ok(())
}

// TEST: GW-I-105
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_105_only_the_profiles_engine_and_only_what_its_exposure_allows(
    pool: PgPool,
) -> TestResult {
    let engine = FakeEngineServer::start(vec![]).await?;
    support::run_configured(pool, None, &config(&engine), async |f| {
        let admin = f.admin().await?;
        // A conversation whose profile names no engine, or another one.
        for named in [None, Some("other")] {
            let (_, token, _) = caller(f, &admin, named).await?;
            let (status, _) = consult(
                f,
                &token,
                "/relay/d/router/v1/before_step",
                &request("before_step"),
            )
            .await?;
            assert_eq!(status, 403, "{named:?}");
        }
        let (_, token, _) = caller(f, &admin, Some("router")).await?;
        assert_eq!(
            consult(
                f,
                &token,
                "/relay/d/router/v1/elsewhere",
                &request("before_step")
            )
            .await?
            .0,
            404
        );
        // Exposure `none`: no arguments, no response text, no transcript, no text in a call's name.
        let mut arguments = request("after_response");
        arguments["response_calls"] =
            json!([{"id":"a","name":"read_file","arguments":{"path":"secret.txt"}}]);
        let mut text = request("after_response");
        text["response_text"] = json!("reading the secret");
        let mut transcript = request("before_step");
        transcript["transcript"] = json!([{"role":"user","text":"task"}]);
        // A call's name is a tool name, so it can't carry the conversation's text either.
        let mut named = request("after_response");
        named["response_calls"] = json!([{"id":"a","name":"the secret is in notes.txt"}]);
        let mut previous = request("before_step");
        previous["previous_calls"] = json!([{"name":"task: rotate the keys"}]);
        for (path, body) in [
            ("/relay/d/router/v1/after_response", &arguments),
            ("/relay/d/router/v1/after_response", &text),
            ("/relay/d/router/v1/before_step", &transcript),
            ("/relay/d/router/v1/after_response", &named),
            ("/relay/d/router/v1/before_step", &previous),
        ] {
            assert_eq!(consult(f, &token, path, body).await?.0, 400, "{body}");
        }
        // A point that differs from the path, and a body that isn't decision/v1.
        assert_eq!(
            consult(
                f,
                &token,
                "/relay/d/router/v1/before_step",
                &request("after_response")
            )
            .await?
            .0,
            400
        );
        assert_eq!(
            consult(f, &token, "/relay/d/router/v1/before_step", &json!({"x":1}))
                .await?
                .0,
            400
        );
        assert!(engine.requests().is_empty(), "nothing was forwarded");
        Ok(())
    })
    .await?;
    engine.stop().await;
    Ok(())
}

// TEST: GW-I-105
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_105_consultations_count_against_the_owners_relay_rate(pool: PgPool) -> TestResult {
    let engine = FakeEngineServer::start(vec![EngineAnswer::Decide(Response::CallModel {
        route: "local".into(),
        tools: vec!["read_file".into()],
    })])
    .await?;
    let configured = format!(
        "{}[relay]\ncapacity=1\nrefill_per_second=0.001\n",
        config(&engine)
    );
    support::run_configured(pool, None, &configured, async |f| {
        let admin = f.admin().await?;
        let (_, token, _) = caller(f, &admin, Some("router")).await?;
        let path = "/relay/d/router/v1/before_step";
        assert_eq!(
            consult(f, &token, path, &request("before_step")).await?.0,
            200
        );
        let (status, body) = consult(f, &token, path, &request("before_step")).await?;
        assert_eq!(status, 429, "{body}");
        assert_eq!(
            engine.requests().len(),
            1,
            "the refused consultation was not forwarded"
        );
        Ok(())
    })
    .await?;
    engine.stop().await;
    Ok(())
}

// TEST: GW-I-105
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_105_bounds_failures_and_the_deadline_are_typed(pool: PgPool) -> TestResult {
    let engine = FakeEngineServer::start(vec![
        EngineAnswer::Raw {
            status: 500,
            body: Vec::new(),
        },
        EngineAnswer::Raw {
            status: 200,
            body: vec![b'x'; 70 * 1024],
        },
        EngineAnswer::Hang,
    ])
    .await?;
    support::run_configured(pool, None, &config(&engine), async |f| {
        let admin = f.admin().await?;
        let (_, token, _) = caller(f, &admin, Some("router")).await?;
        let path = "/relay/d/router/v1/before_step";
        let mut big = request("before_step");
        big["tools"][0]["description"] = json!("d".repeat(70 * 1024));
        assert_eq!(
            consult(f, &token, path, &big).await?.0,
            413,
            "an oversized request"
        );
        assert!(engine.requests().is_empty());
        assert_eq!(
            consult(f, &token, path, &request("before_step")).await?.0,
            502,
            "a failed engine"
        );
        assert_eq!(
            consult(f, &token, path, &request("before_step")).await?.0,
            502,
            "an oversized answer"
        );
        let started = std::time::Instant::now();
        assert_eq!(
            consult(f, &token, path, &request("before_step")).await?.0,
            504,
            "past timeout_ms"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        Ok(())
    })
    .await?;
    engine.stop().await;
    Ok(())
}

// TEST: GW-I-105
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_105_a_message_under_a_removed_engine_is_refused(pool: PgPool) -> TestResult {
    let engine = FakeEngineServer::start(vec![]).await?;
    support::run_configured(pool, None, &config(&engine), async |f| {
        let admin = f.admin().await?;
        let (conversation, _, cookie) = caller(f, &admin, Some("gone")).await?;
        let path = format!("/api/v1/conversations/{}/messages", conversation.id);
        let refused = f
            .action(
                &path,
                &cookie,
                Some("engine-gone"),
                &json!({"text":"start"}),
            )
            .await?;
        assert_eq!(refused.status(), 409);
        let body: Value = refused.json().await?;
        assert_eq!(
            body["error"]["code"], "decision_engine_unavailable",
            "{body}"
        );
        Ok(())
    })
    .await?;
    engine.stop().await;
    Ok(())
}
