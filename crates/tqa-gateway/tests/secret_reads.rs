//! Secret read audit (ADR-0033).
//! - GW-I-094: every read of a secret is recorded, aggregated per reference, consumer, conversation, user, run and
//!   outcome, never with a value; an administrator sees a shared secret's reads, a user sees their own credential's
//!   reads.
//! - GW-I-095: read records past `read_audit_days` and administration records past `admin_audit_days` are swept.
//! - GW-I-147: no key is used without its audit row (D4): the row is committed before the key is returned, a full
//!   queue makes the read wait and never drops it, and a read whose row can't be written fails and the key isn't used.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use support::{Fixture, TestResult, call};
use tower::ServiceExt as _;
use tqa_testkit::{FakeModelServer, Script, Scripted};

const CONNECTORS: &str = "[[connectors]]\nname='tracker'\nurl='https://mcp.tracker.example/mcp'\n\
    auth={scheme='bearer'}\nsecret_ref='secrets/tracker-shared'\n";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='m'\nauth={{kind='bearer'}}\n\
         key_ref='secrets/local'\n",
        server.base_url()
    ))?)
}

async fn completion(f: &Fixture, token: &secrecy::SecretString, run: &str) -> Result<u16> {
    Ok(f.client
        .post(f.url("/relay/r/local/chat/completions"))
        .bearer_auth(token.expose_secret())
        .header("X-TQA-Run-ID", run)
        .json(&json!({"messages":[{"role":"user","content":"hi"}],"stream":true}))
        .send()
        .await?
        .status()
        .as_u16())
}

/// The kind, consumer, run, outcome and count of every read record of `reference`, sorted.
async fn records(
    f: &Fixture,
    reference: &str,
) -> Result<Vec<(String, String, Option<String>, String, i64)>> {
    f.gateway.read_audit_flush_now().await?;
    Ok(sqlx::query_as(
        "SELECT consumer_kind, consumer, run_id, outcome, count FROM secret_reads WHERE ref=$1 ORDER BY run_id, outcome",
    )
    .bind(reference)
    .fetch_all(&f.pool)
    .await?)
}

fn row(
    kind: &str,
    consumer: &str,
    run: Option<&str>,
    outcome: &str,
    count: i64,
) -> (String, String, Option<String>, String, i64) {
    (
        kind.into(),
        consumer.into(),
        run.map(str::to_owned),
        outcome.into(),
        count,
    )
}

async fn metric(f: &Fixture, name: &str) -> Result<String> {
    let response = f
        .gateway
        .metrics_router()
        .oneshot(axum::http::Request::get("/metrics").body(axum::body::Body::empty())?)
        .await?;
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await?;
    Ok(String::from_utf8(bytes.to_vec())?
        .lines()
        .find(|line| line.starts_with(name))
        .unwrap_or_default()
        .to_owned())
}

// TEST: GW-I-094
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_094_reads_are_audited_aggregated_and_shown_never_with_a_value(
    pool: PgPool,
) -> TestResult {
    let script = Script::new(vec![Scripted::Text("ok".into())]);
    let server = FakeModelServer::start(vec![script; 4], Arc::new(tqa_llm::TokioSleeper)).await?;
    let config = format!("{CONNECTORS}[secret_store]\nread_audit_queue=4\n");
    let result = support::run_configured(pool, Some(registry(&server)?), &config, async |f| {
        let (alice, admin) = f.admin().await?;
        let conversation = f.conversation(alice.id).await?;
        // The runs the calls name are the task's own: only those are recorded (GW-I-151).
        for (seq, run) in [(1, "run-1"), (2, "run-2"), (3, "run-3")] {
            f.gateway
                .events()
                .append(&conversation.id, support::started(seq, run)?)
                .await?;
        }
        let token = f
            .gateway
            .runtime_tokens()
            .mint(
                &conversation.id,
                &"reads".parse()?,
                std::net::Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        let value = format!("synthetic-provider-{}", uuid::Uuid::now_v7());
        f.gateway
            .secret_store()
            .put(&"secrets/local".parse()?, value.clone().into())
            .await?;
        // Two calls in one run and one in another: two records, the first counted twice.
        for run in ["run-1", "run-1", "run-2"] {
            assert_eq!(completion(f, &token, run).await?, 200);
        }
        assert_eq!(
            records(f, "secrets/local").await?,
            [
                row("route", "local", Some("run-1"), "ok", 2),
                row("route", "local", Some("run-2"), "ok", 1)
            ]
        );
        let (conversation_id, user_id): (uuid::Uuid, uuid::Uuid) = sqlx::query_as(
            "SELECT conversation_id, user_id FROM secret_reads WHERE ref='secrets/local' LIMIT 1",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(
            (conversation_id.to_string(), user_id),
            (conversation.id.as_str().to_owned(), alice.id)
        );
        // A read that finds no value is recorded as absent (and the request fails closed).
        f.gateway
            .secret_store()
            .delete(&"secrets/local".parse()?)
            .await?;
        assert_ne!(completion(f, &token, "run-3").await?, 200);
        assert!(records(f, "secrets/local").await?.contains(&row(
            "route",
            "local",
            Some("run-3"),
            "absent",
            1
        )));
        // Connector reads: a user's own credential, and the shared one for a user without their own.
        let (bob, bob_cookie) = f.member("bob@example.invalid").await?;
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/connectors/tracker/credential",
            &bob_cookie,
            (Some(json!({"value": "synthetic-bob"})), None),
        )
        .await?;
        assert_eq!(status, 204);
        f.gateway
            .secret_store()
            .put(
                &"secrets/tracker-shared".parse()?,
                "synthetic-shared".into(),
            )
            .await?;
        f.gateway.connector_credential(bob.id, "tracker").await?;
        f.gateway.connector_credential(bob.id, "tracker").await?;
        f.gateway.connector_credential(alice.id, "tracker").await?;
        assert_eq!(
            records(f, "secrets/tracker-shared").await?,
            [row("connector", "tracker", None, "ok", 1)]
        );
        // The administrator's view of a shared secret, newest first, never a value.
        let (status, reads) = call(
            f,
            Method::GET,
            "/api/v1/admin/secrets/local/reads",
            &admin,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200, "{reads}");
        let items = reads["items"].as_array().ok_or("items")?;
        assert_eq!(items.len(), 3, "{reads}");
        assert_eq!(items[0]["run_id"], "run-3", "newest first: {reads}");
        assert_eq!(items[0]["outcome"], "absent");
        assert!(!reads.to_string().contains(&value));
        // A user sees only their own credential's reads, and the administrator cannot ask for them by name.
        let (status, own) = call(
            f,
            Method::GET,
            "/api/v1/connectors/tracker/credential/reads",
            &bob_cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200, "{own}");
        let own = own["items"].as_array().ok_or("items")?;
        assert_eq!(
            (own.len(), &own[0]["count"], &own[0]["consumer"]),
            (1, &json!(2), &json!("tracker"))
        );
        assert!(
            !Value::Array(own.clone())
                .to_string()
                .contains("synthetic-bob")
        );
        let (status, _) = call(
            f,
            Method::GET,
            "/api/v1/connectors/tracker/credential/reads",
            &admin,
            (None, None),
        )
        .await?;
        assert_eq!(
            status, 404,
            "alice has no credential of her own for tracker"
        );
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

/// The reads of `user`'s own tracker credential that were recorded as used.
async fn used(f: &Fixture, user: uuid::Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0)::bigint FROM secret_reads WHERE consumer_kind='connector' AND user_id=$1 AND outcome='ok'",
    )
    .bind(user)
    .fetch_one(&f.pool)
    .await?)
}

// TEST: GW-I-147
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_147_no_key_is_used_without_its_committed_audit_row(pool: PgPool) -> TestResult {
    let config = format!("{CONNECTORS}[secret_store]\nread_audit_queue=4\n");
    support::run_configured(pool, None, &config, async |f| {
        f.admin().await?;
        let (bob, bob_cookie) = f.member("bob@example.invalid").await?;
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/connectors/tracker/credential",
            &bob_cookie,
            (Some(json!({"value": "synthetic-bob"})), None),
        )
        .await?;
        assert!((200..300).contains(&status));
        // The row is committed before the key is returned: it is there with no flush.
        assert!(
            f.gateway
                .connector_credential(bob.id, "tracker")
                .await?
                .is_some()
        );
        assert_eq!(used(f, bob.id).await?, 1);
        // A full queue (4) makes reads wait for room; none is dropped, and each key comes back once recorded.
        f.gateway.read_audit_hold(true);
        let reads = futures::future::join_all(
            (0..8).map(|_| f.gateway.connector_credential(bob.id, "tracker")),
        );
        let release = async {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            f.gateway.read_audit_hold(false);
        };
        let (answers, ()) = tokio::join!(reads, release);
        for answer in answers {
            assert!(answer?.is_some());
        }
        assert_eq!(
            used(f, bob.id).await?,
            9,
            "every read recorded, none dropped"
        );
        // A row that can't be written: the key is not used, and the call fails with plain words.
        sqlx::query("ALTER TABLE secret_reads RENAME TO secret_reads_away")
            .execute(&f.pool)
            .await?;
        let refused = f.gateway.connector_credential(bob.id, "tracker").await;
        sqlx::query("ALTER TABLE secret_reads_away RENAME TO secret_reads")
            .execute(&f.pool)
            .await?;
        let error = refused
            .err()
            .ok_or("the key was returned without its audit row")?;
        assert_eq!(
            (error.code(), error.to_string().as_str()),
            (
                "key_use_unrecorded",
                "The key could not be used: its use could not be recorded. Try again."
            )
        );
        assert_eq!(
            metric(f, "tqa_secret_reads_unrecorded_total ").await?,
            "tqa_secret_reads_unrecorded_total 1",
            "the refused use is counted"
        );
        // The cost: 100 reads one after another, each waiting for its row.
        let mut took = Vec::with_capacity(100);
        for _ in 0..100 {
            let started = std::time::Instant::now();
            f.gateway.connector_credential(bob.id, "tracker").await?;
            took.push(started.elapsed());
        }
        took.sort_unstable();
        let (median, p99) = (took[49], took[98]);
        if let Ok(path) = std::env::var("TQA_READ_AUDIT_LATENCY") {
            std::fs::write(path, format!("100 reads: p50 {median:?}, p99 {p99:?}\n"))?;
        }
        assert!(
            p99 < tqa_testkit::deadline::scaled(std::time::Duration::from_secs(1)),
            "p99 {p99:?}"
        );
        assert_eq!(used(f, bob.id).await?, 109);
        Ok(())
    })
    .await
}

// TEST: GW-I-095
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_095_old_read_and_administration_records_are_swept(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[secret_store]\nread_audit_days=30\nadmin_audit_days=365\n", async |f| {
        let (_, cookie) = f.admin().await?;
        let (status, _) = call(f, Method::PUT, "/api/v1/admin/secrets/provider", &cookie, (Some(json!({"value": "v"})), None)).await?;
        assert_eq!(status, 201);
        for (days, run) in [(31, "old"), (29, "recent")] {
            sqlx::query(
                "INSERT INTO secret_reads (id,ref,consumer_kind,consumer,run_id,outcome,first_at,last_at,count) \
                 VALUES ($1,'secrets/provider','route','local',$2,'ok',CURRENT_TIMESTAMP - make_interval(days => $3), \
                 CURRENT_TIMESTAMP - make_interval(days => $3),1)",
            )
            .bind(uuid::Uuid::now_v7())
            .bind(run)
            .bind(days)
            .execute(&f.pool)
            .await?;
        }
        // Every record so far (the settings seed's too) is past the administration retention.
        let aged = sqlx::query("UPDATE admin_audit SET at = CURRENT_TIMESTAMP - make_interval(days => 366)")
            .execute(&f.pool)
            .await?
            .rows_affected();
        assert!(aged >= 1);
        let (status, _) = call(f, Method::PUT, "/api/v1/admin/secrets/other", &cookie, (Some(json!({"value": "v"})), None)).await?;
        assert_eq!(status, 201);
        let (reads, audit) = f.gateway.audit_retention_now().await?;
        assert_eq!((reads, audit), (1, aged));
        let left: Vec<String> = sqlx::query_scalar("SELECT run_id FROM secret_reads").fetch_all(&f.pool).await?;
        assert_eq!(left, ["recent"]);
        let kept: Vec<String> = sqlx::query_scalar("SELECT item FROM admin_audit").fetch_all(&f.pool).await?;
        assert_eq!(kept, ["secrets/other"]);
        Ok(())
    })
    .await
}
