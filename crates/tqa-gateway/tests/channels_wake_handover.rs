//! A channel's wake, handed over (ADR-0014's amendment of 20 Nov 2026, reviewed on 20 Nov 2026): the wake is the
//! gateway's tracked task, stopped by its cancel (GW-I-360); never handed to a runtime below 16 (GW-I-361); and the run
//! it starts is charged to it, not to whichever turn was touched last (GW-I-371).
#[path = "support/channel_agent.rs"]
mod channel_agent;
#[path = "support/fixture.rs"]
mod support;
use channel_agent::{Agent, QUIET, WAKE, channel, notices, post, text};
use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use support::{TestResult, call};
use tqa_types::ClientFrame;
use uuid::Uuid;

// TEST: GW-I-360
// A wake's send belongs to the gateway's shutdown (review of 20 Nov 2026): it is one of the gateway's tracked tasks,
// and cancelled before the wake gathers its burst, the gateway queues no wake at all.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_360_a_cancelled_gateway_sends_no_wake(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let analyst = Agent::new(f, owner.id, "Pricing analyst").await?;
        let id = channel(f, &cookie, "pricing", &[(&analyst, "member")]).await?;
        // The wake gathering its burst is the gateway's own task, which its shutdown joins.
        let before = f.gateway.tracked_tasks();
        post(f, &cookie, &id, text("@pricing analyst look"), None).await?;
        assert_eq!(f.gateway.tracked_tasks(), before + 1, "the wake is a tracked task");
        f.gateway.cancel();
        tokio::time::sleep(Duration::from_millis(2500)).await;
        let queued: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency WHERE conversation_id = $1 AND key LIKE 'channel-wake:%'",
        )
        .bind(analyst.uuid)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(queued, 0, "a cancelled gateway queued a wake");
        Ok(())
    }))
    .await
}

// TEST: GW-I-361
// A wake never reaches a runtime below 16 (review of 20 Nov 2026): a member never linked before is woken, its sandbox
// starts, and its runtime announces 15. The wake isn't handed over (it couldn't parse it); the channel says once that
// the member can't be woken, and the member's own work goes on as before.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_361_a_wake_is_never_handed_to_a_runtime_below_16(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let mut old = Agent::new(f, owner.id, "Old runtime").await?;
        let id = channel(f, &cookie, "pricing", &[(&old, "member")]).await?;
        post(f, &cookie, &id, text("@old runtime look"), None).await?;
        // Its sandbox starts for the wake; its runtime links at 15 and is handed nothing.
        let next = old.next_at(f, QUIET, 15).await?;
        assert!(next.is_none(), "a runtime below 16 was handed {next:?}");
        assert_eq!(notices(f, &id, "unreachable_runtime").await?, 1);
        post(f, &cookie, &id, text("@old runtime again"), None).await?;
        assert!(old.next(f, QUIET).await?.is_none());
        assert_eq!(
            notices(f, &id, "unreachable_runtime").await?,
            1,
            "said once"
        );
        // The person's own message still runs it.
        let (status, _) = call(
            f,
            Method::POST,
            &format!("/api/v1/conversations/{}/messages", old.id),
            &cookie,
            (Some(json!({"text": "Start."})), Some("person-1")),
        )
        .await?;
        assert!(status < 300, "{status}");
        let Some(ClientFrame::UserMessage {
            channel_wake: None, ..
        }) = old.next(f, WAKE).await?
        else {
            return Err("the person's own message".into());
        };
        Ok(())
    }))
    .await
}

// TEST: GW-I-371
// A woken run is charged to the wake handed over for it (CI on #144, 20 Nov 2026): never to whichever delivered turn
// was touched last. Alpha's first wake's row is given the later update time just before its second woken run starts;
// that run's post is still one hop past the wake that started it (3), not past the first (1).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_371_a_woken_run_is_charged_to_the_wake_handed_over_for_it(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let mut alpha = Agent::new(f, owner.id, "Alpha").await?;
        let mut beta = Agent::new(f, owner.id, "Beta").await?;
        let id = channel(f, &cookie, "loop", &[(&alpha, "member"), (&beta, "member")]).await?;
        post(f, &cookie, &id, text("@alpha start"), None).await?;
        alpha.woken(f, &id, 0).await?;
        alpha.start(f).await?;
        let at = *alpha.read(f, &id, 0).await?.last().ok_or("read")?;
        alpha.post(f, &id, text("@beta hop 1")).await?;
        alpha.end(f).await?;
        beta.woken(f, &id, 0).await?;
        beta.start(f).await?;
        beta.read(f, &id, 0).await?;
        beta.post(f, &id, text("@alpha hop 2")).await?;
        beta.end(f).await?;
        alpha.woken(f, &id, at).await?;
        // Alpha's first wake's row is touched after its second was delivered, as a run's end or a retry may do.
        sqlx::query(
            "UPDATE idempotency SET updated_at = CURRENT_TIMESTAMP + interval '1 hour' \
             WHERE conversation_id = $1 AND key LIKE 'channel-wake:%' AND key LIKE '%:0'",
        )
        .bind(alpha.uuid)
        .execute(&f.pool)
        .await?;
        alpha.start(f).await?;
        alpha.read(f, &id, at).await?;
        let seq = alpha.post(f, &id, text("@beta hop 3")).await?;
        let hop: i16 = sqlx::query_scalar(
            "SELECT hop FROM channel_messages WHERE channel_id = $1 AND seq = $2",
        )
        .bind(Uuid::parse_str(&id)?)
        .bind(i64::try_from(seq)?)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(
            hop, 3,
            "charged to the wake that started this run (hop 2), not the first (hop 0)"
        );
        Ok(())
    }))
    .await
}
