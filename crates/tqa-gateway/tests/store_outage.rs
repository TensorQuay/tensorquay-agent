//! GW-E-020: an isolated database outage preserves the sandbox and the runtime's replay tail.
#[path = "support/fixture.rs"]
mod support;

use futures::FutureExt;
use secrecy::ExposeSecret;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{panic::AssertUnwindSafe, sync::atomic::Ordering, time::Duration};
use support::{Fixture, TestResult};
use tqa_types::{ErrorCode, Event, ServerFrame};

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_e_020_database_outage_notifies_once_and_redial_replays_the_unacknowledged_tail(
    pool: PgPool,
) -> TestResult {
    // The link retries a frame's append for three `io_timeout`s before it gives up (GW-U-075): at 2 s, the outage
    // closes it within the fixture's deadline for the runtime's error.
    support::run_configured(pool, None, "[limits]\nio_timeout=2\n", async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let mut stream = f.stream(&conversation.id, &cookie, 0).await?;
        runtime
            .send(&event(
                1,
                Event::RunStarted {
                    run_id: "store-outage".parse()?,
                },
            ))
            .await?;
        assert_eq!(stream.durable().await?.0, 1.into());
        assert_eq!(stream.state().await?, tqa_types::SandboxState::Busy);
        f.clock.freeze();
        f.clock.observed().await?;
        let wal: Vec<_> = (2..=32)
            .map(|seq| support::frame(seq, "buffered in runtime WAL"))
            .collect();
        unavailable(f, async {
            for frame in &wal {
                runtime.send(frame).await?;
            }
            assert_eq!(runtime.error().await?, ErrorCode::StoreUnavailable);
            runtime.closed().await?;
            assert!(matches!(
                stream.next().await?.1,
                ServerFrame::Error {
                    code: ErrorCode::StoreUnavailable,
                    seq: None,
                    ..
                }
            ));
            f.clock.advance(Duration::from_secs(30));
            assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 0);
            Ok(())
        })
        .await?;
        let mut redial = f.runtime(&conversation).await?;
        assert_eq!(redial.config["last_stored_seq"], 1);
        for pending in &wal {
            redial.send(pending).await?;
        }
        for pending in &wal {
            assert_eq!(stream.durable().await?.1, *pending);
        }
        redial
            .send(&event(
                33,
                Event::RunCompleted {
                    run_id: "store-outage".parse()?,
                },
            ))
            .await?;
        assert_eq!(stream.durable().await?.0, 33.into());
        f.clock.observed().await.map_err(
            |_| "manager did not resume lifecycle observation after the store recovered",
        )?;
        f.clock.observed().await?;
        assert_eq!(f.backend.creates.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.gateway
                .events()
                .read(&conversation.id, 0.into(), 100)
                .await?
                .len(),
            33
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_e_020_relay_answers_retryable_503_never_an_auth_error_during_a_store_outage(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let token = f
            .gateway
            .runtime_tokens()
            .mint(
                &conversation.id,
                &"outage-relay-sandbox".parse()?,
                std::net::Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        // A well-formed token the store does not know: only the store can refute it.
        let mut unknown = token.expose_secret().to_owned();
        let last = if unknown.ends_with('a') { 'b' } else { 'a' };
        unknown.pop();
        unknown.push(last);
        unavailable(f, async {
            // A valid token cannot be confirmed and an unknown one cannot be refuted while the store is down.
            for bearer in [token.expose_secret().to_owned(), unknown.clone()] {
                let response = f
                    .client
                    .post(f.url("/relay/r/local/chat/completions"))
                    .bearer_auth(bearer)
                    .json(&serde_json::json!({"messages":[{"role":"user","content":"hello"}],"stream":true}))
                    .send()
                    .await?;
                assert_eq!(response.status(), 503);
                assert_eq!(
                    response.headers().get("retry-after").ok_or("missing Retry-After")?,
                    "1"
                );
                let body: serde_json::Value = response.json().await?;
                assert_eq!(body["error"]["code"], "store_unavailable");
                assert_eq!(body["error"]["retryable"], true);
            }
            Ok(())
        })
        .await
    })
    .await
}

async fn unavailable(f: &Fixture, operation: impl Future<Output = TestResult>) -> TestResult {
    let name = f
        .pool
        .connect_options()
        .get_database()
        .ok_or("database name")?
        .to_owned();
    assert!(
        name.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let admin = tokio::time::timeout(
        support::DEADLINE,
        PgPoolOptions::new()
            .max_connections(1)
            .connect_with((*f.pool.connect_options()).clone().database("postgres")),
    )
    .await??;
    let outcome = AssertUnwindSafe(async {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER DATABASE \"{name}\" ALLOW_CONNECTIONS false"
        )))
        .execute(&admin)
        .await?;
        sqlx::query("SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname=$1")
            .bind(&name)
            .execute(&admin)
            .await?;
        operation.await
    })
    .catch_unwind()
    .await;
    let restore = sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER DATABASE \"{name}\" ALLOW_CONNECTIONS true"
    )))
    .execute(&admin)
    .await;
    admin.close().await;
    restore?;
    match outcome {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    }
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
