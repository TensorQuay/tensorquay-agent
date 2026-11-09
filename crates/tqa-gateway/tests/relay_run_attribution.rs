//! GW-I-151: which run a relayed model call is for, as the key-read audit and the usage row record it. The runtime is
//! untrusted: the run its `X-TQA-Run-ID` names is kept only when it is a run of the caller's own conversation (its
//! `run.started` is stored, or is stored within a short wait), else it is recorded as none and counted in
//! `tqa_relay_unattributed_total`; it never authorizes the call. A malformed header is still refused.
#[path = "support/fixture.rs"]
mod support;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use support::{Fixture, TestResult};
use tower::ServiceExt as _;
use tqa_testkit::{FakeModelServer, Script, Scripted};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='m'\nauth={{kind='bearer'}}\n\
         key_ref='secrets/local'\n",
        server.base_url()
    ))?)
}

async fn completion(f: &Fixture, token: &SecretString, run: Option<&str>) -> Result<u16> {
    let mut request = f
        .client
        .post(f.url("/relay/r/local/chat/completions"))
        .bearer_auth(token.expose_secret())
        .json(&json!({"messages":[{"role":"user","content":"hi"}],"stream":true}));
    if let Some(run) = run {
        request = request.header("X-TQA-Run-ID", run);
    }
    Ok(request.send().await?.status().as_u16())
}

/// The run of every key read and every usage row of the conversation, in order.
async fn recorded(
    f: &Fixture,
    conversation: uuid::Uuid,
) -> Result<(Vec<Option<String>>, Vec<Option<String>>)> {
    f.gateway.read_audit_flush_now().await?;
    let reads = sqlx::query_scalar(
        "SELECT run_id FROM secret_reads WHERE conversation_id=$1 AND consumer_kind='route' ORDER BY run_id NULLS FIRST",
    )
    .bind(conversation)
    .fetch_all(&f.pool)
    .await?;
    let usage = sqlx::query_scalar(
        "SELECT run_id FROM usage WHERE conversation_id=$1 ORDER BY run_id NULLS FIRST",
    )
    .bind(conversation)
    .fetch_all(&f.pool)
    .await?;
    Ok((reads, usage))
}

async fn metric(f: &Fixture, reason: &str) -> Result<String> {
    let response = f
        .gateway
        .metrics_router()
        .oneshot(axum::http::Request::get("/metrics").body(axum::body::Body::empty())?)
        .await?;
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await?;
    Ok(String::from_utf8(bytes.to_vec())?
        .lines()
        .find(|line| {
            line.starts_with("tqa_relay_unattributed_total{")
                && line.contains(&format!("reason=\"{reason}\""))
        })
        .unwrap_or_default()
        .to_owned())
}

async fn token(
    f: &Fixture,
    conversation: &tqa_types::ConversationId,
    name: &str,
) -> Result<SecretString> {
    Ok(f.gateway
        .runtime_tokens()
        .mint(
            conversation,
            &name.parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?)
}

// TEST: GW-I-151
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_151_only_a_run_of_the_callers_own_task_is_recorded(pool: PgPool) -> TestResult {
    let script = Script::new(vec![Scripted::Text("ok".into())]);
    let server = FakeModelServer::start(vec![script; 5], Arc::new(tqa_llm::TokioSleeper)).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        let (alice, _) = f.admin().await?;
        let mine = f.conversation(alice.id).await?;
        let other = f.conversation(alice.id).await?;
        f.gateway
            .secret_store()
            .put(
                &"secrets/local".parse()?,
                "synthetic-provider".to_owned().into(),
            )
            .await?;
        // Stored and published as the runtime's own frames are.
        f.gateway
            .events()
            .append(&mine.id, support::started(1, "run-mine")?)
            .await?;
        f.gateway
            .events()
            .append(&other.id, support::started(1, "run-other")?)
            .await?;
        let relay = token(f, &mine.id, "attribution-relay").await?;
        // Its own run is kept; another task's run and no run at all are recorded as none, and counted. A run the
        // store already holds adds no wait; one it doesn't waits at most the 1 s bound for its run.started. No run at
        // all goes first: it pays the first call's one-time costs (the route, the key, the model server's connection),
        // so the two calls measured next differ only in the wait.
        assert_eq!(completion(f, &relay, None).await?, 200);
        let started_at = std::time::Instant::now();
        assert_eq!(completion(f, &relay, Some("run-mine")).await?, 200);
        let hit = started_at.elapsed();
        let started_at = std::time::Instant::now();
        assert_eq!(completion(f, &relay, Some("run-other")).await?, 200);
        let miss = started_at.elapsed();
        // A known run is measured against the unknown one, which waits the 1 s bound: on a slow (instrumented)
        // runner both calls cost more, but only a wait for run.started puts the known one within 500 ms of it.
        assert!(
            hit + std::time::Duration::from_millis(500) < miss,
            "a known run waited: {hit:?} (an unknown one: {miss:?})"
        );
        assert!(
            miss >= std::time::Duration::from_secs(1)
                && miss < tqa_testkit::deadline::scaled(std::time::Duration::from_millis(1500)),
            "an unknown run waits the 1 s bound, no longer: {miss:?}"
        );
        // A run whose run.started is stored just after its first call still counts.
        let late = tokio::spawn({
            let (client, url, relay) = (
                f.client.clone(),
                f.url("/relay/r/local/chat/completions"),
                relay.clone(),
            );
            async move {
                client
                    .post(url)
                    .bearer_auth(relay.expose_secret())
                    .header("X-TQA-Run-ID", "run-late")
                    .json(&json!({"messages":[{"role":"user","content":"hi"}],"stream":true}))
                    .send()
                    .await
                    .map(|response| response.status().as_u16())
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        f.gateway
            .events()
            .append(&mine.id, support::started(2, "run-late")?)
            .await?;
        assert_eq!(late.await??, 200);
        // A malformed id is refused as before, and reaches nothing.
        assert_eq!(completion(f, &relay, Some("not a run id")).await?, 400);
        let conversation = uuid::Uuid::parse_str(&mine.id.to_string())?;
        let expected = vec![
            None,
            None,
            Some("run-late".to_owned()),
            Some("run-mine".to_owned()),
        ];
        let (reads, usage) = recorded(f, conversation).await?;
        assert_eq!(usage, expected, "usage rows");
        // Key reads aggregate by run: the two unattributed calls share one row.
        assert_eq!(
            reads,
            vec![
                None,
                Some("run-late".to_owned()),
                Some("run-mine".to_owned())
            ],
            "key reads"
        );
        assert!(
            metric(f, "foreign").await?.ends_with(" 1"),
            "{}",
            metric(f, "foreign").await?
        );
        assert!(
            metric(f, "missing").await?.ends_with(" 1"),
            "{}",
            metric(f, "missing").await?
        );
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}
