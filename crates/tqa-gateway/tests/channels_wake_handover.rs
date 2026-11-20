//! A channel's wake, handed over (ADR-0014's amendment of 20 Nov 2026, reviewed on 20 Nov 2026): the wake is the
//! gateway's tracked task, stopped by its cancel (GW-I-360); never handed to a runtime below 16 (GW-I-361), whose
//! sandbox from before the wake it replaces instead (GW-I-382); and the run it starts is charged to it, not to whichever
//! turn was touched last (GW-I-371). An upgraded install wakes no one for posts made before it (GW-I-383).
#[path = "support/channel_agent.rs"]
mod channel_agent;
#[path = "support/fixture.rs"]
mod support;
use channel_agent::{Agent, QUIET, WAKE, channel, notices, post, replaced, runtime_call, text};
use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
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

// TEST: GW-I-379
// A woken run that ends badly is said in the channel (decided on 20 Nov 2026, C-6): one notice per run, naming the
// member and, for a failure, its code, so the page words it as the task page does; a run no wake started tells the
// channel nothing.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_379_a_woken_run_that_fails_or_stops_is_said_in_the_channel(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let mut analyst = Agent::new(f, owner.id, "Pricing analyst").await?;
        let id = channel(f, &cookie, "pricing", &[(&analyst, "member")]).await?;
        let body = |kind: &'static str| {
            let pool = f.pool.clone();
            let id = id.clone();
            async move {
                sqlx::query_scalar::<_, serde_json::Value>(
                    "SELECT body FROM channel_messages WHERE channel_id = $1::uuid AND body->>'kind' = $2 ORDER BY seq",
                )
                .bind(&id)
                .bind(kind)
                .fetch_all(&pool)
                .await
            }
        };
        // A woken run that fails.
        let first = post(f, &cookie, &id, text("@Pricing analyst churn by plan, please"), None).await?;
        analyst.woken(f, &id, 0).await?;
        analyst.start(f).await?;
        analyst.read(f, &id, 0).await?;
        analyst.fail(f).await?;
        let failed = body("woken_run_failed").await?;
        assert_eq!(failed.len(), 1, "{failed:?}");
        assert_eq!((failed[0]["member"].clone(), failed[0]["code"].clone()), (json!("Pricing analyst"), json!("reply_cut")));
        // A woken run that is stopped.
        post(f, &cookie, &id, text("@Pricing analyst and by region"), None).await?;
        analyst.woken(f, &id, first).await?;
        analyst.start(f).await?;
        let run_id = format!("r-{}", analyst.runs).parse()?;
        analyst.event(f, tqa_types::Event::RunCancelled { run_id }).await?;
        let stopped = body("woken_run_stopped").await?;
        assert_eq!(stopped.len(), 1, "{stopped:?}");
        assert_eq!(stopped[0]["member"], json!("Pricing analyst"));
        // A run its person started, failing, tells the channel nothing.
        analyst.start(f).await?;
        analyst.fail(f).await?;
        assert_eq!(body("woken_run_failed").await?.len(), 1);
        assert_eq!(notices(f, &id, "woken_run_stopped").await?, 1);
        Ok(())
    }))
    .await
}

// TEST: GW-I-380
// A woken run never parks in channel_wait (decided on 20 Nov 2026, C-11): its wait answers at once with what is new,
// as a read does, and says it was woken, so the agent ends its turn and is woken again when more arrives. A run its
// person started waits as before.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_380_a_woken_run_never_parks_in_a_wait(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let mut analyst = Agent::new(f, owner.id, "Pricing analyst").await?;
        let id = channel(f, &cookie, "pricing", &[(&analyst, "member")]).await?;
        let asked = post(
            f,
            &cookie,
            &id,
            text("@Pricing analyst churn by plan"),
            None,
        )
        .await?;
        analyst.woken(f, &id, 0).await?;
        analyst.start(f).await?;
        let token = analyst.token(f)?;
        let waited = |since: u64| {
            let token = token.clone();
            let id = id.clone();
            async move {
                let started = std::time::Instant::now();
                let (status, page) = runtime_call(
                    f,
                    Method::GET,
                    &format!("/runtime/channels/{id}/messages?since={since}&wait=30"),
                    &token,
                    None,
                )
                .await?;
                Ok::<_, Box<dyn std::error::Error>>((status, page, started.elapsed()))
            }
        };
        // In the woken run: at once, with what is new, and marked.
        let (status, page, took) = waited(0).await?;
        assert_eq!(status, 200, "{page}");
        assert!(
            took < Duration::from_secs(5),
            "a woken run's wait parked for {took:?}"
        );
        assert_eq!(page["woken"], json!(true), "{page}");
        assert_eq!(page["messages"][0]["seq"], json!(asked), "{page}");
        // Nothing new: still at once, empty, marked.
        let (_, page, took) = waited(asked).await?;
        assert!(took < Duration::from_secs(5), "{took:?}");
        assert_eq!(
            (page["messages"].clone(), page["woken"].clone()),
            (json!([]), json!(true))
        );
        analyst.end(f).await?;
        // A run its person started waits as before (here until a post arrives), unmarked.
        analyst.start(f).await?;
        let later = {
            let cookie = cookie.clone();
            let id = id.clone();
            async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                post(f, &cookie, &id, text("Separately: lunch?"), None).await
            }
        };
        let ((_, page, took), _) = tokio::try_join!(waited(asked), later)?;
        assert!(
            took >= Duration::from_secs(2),
            "a person's run's wait returned early ({took:?})"
        );
        assert!(
            page.get("woken").is_none() || page["woken"] == json!(false),
            "{page}"
        );
        analyst.end(f).await?;
        Ok(())
    }))
    .await
}

// TEST: GW-I-382
// An upgrade needs nothing from people (decided on 21 Nov 2026, C-15): a wake for a member whose sandbox runs a runtime
// below 16 replaces that sandbox, as an administrator's stop does, and goes to the new one; while a run goes, the
// sandbox stays until the run has ended. No "can't be woken" notice when the replacement reads the wake.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_382_a_wake_replaces_a_sandbox_too_old_to_read_it(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let mut old = Agent::new(f, owner.id, "Old runtime").await?;
        let id = channel(f, &cookie, "pricing", &[(&old, "member")]).await?;
        // Its sandbox from before the upgrade: the runtime announces 15, and a run of its own is going.
        let (status, _) = call(
            f,
            Method::POST,
            &format!("/api/v1/conversations/{}/messages", old.id),
            &cookie,
            (Some(json!({"text": "Start."})), Some("person-1")),
        )
        .await?;
        assert!(status < 300, "{status}");
        old.next_at(f, WAKE, 15).await?.ok_or("the person's turn")?;
        old.start(f).await?;
        let destroys = f.backend.destroys.load(Ordering::SeqCst);
        let asked = post(f, &cookie, &id, text("@old runtime please look"), None).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            destroys,
            "a run in progress keeps its sandbox"
        );
        // Its run ends: the sandbox is replaced, and the new one's runtime gets the wake.
        old.end(f).await?;
        replaced(f, &mut old, destroys, tqa_types::PROTOCOL_REVISION).await?;
        let sentence = old.woken(f, &id, 0).await?;
        assert!(sentence.contains(&format!("message {asked}")), "{sentence}");
        assert_eq!(
            notices(f, &id, "unreachable_runtime").await?,
            0,
            "no notice when the replacement reads it"
        );
        old.start(f).await?;
        assert_eq!(old.read(f, &id, 0).await?, vec![asked]);
        old.end(f).await?;
        // The member is a member like any other now: the next post wakes it with no replacement.
        let again = post(f, &cookie, &id, text("@old runtime one more"), None).await?;
        let sentence = old.woken(f, &id, asked).await?;
        assert!(sentence.contains(&format!("message {again}")), "{sentence}");
        assert_eq!(
            f.backend.destroys.load(Ordering::SeqCst),
            destroys + 1,
            "one replacement only"
        );
        Ok(())
    }))
    .await
}

// TEST: GW-I-383
// An upgrade wakes no one for posts made before it (18.2, decided on 21 Nov 2026): the release before wakes didn't mark
// whom a post wakes, and migration 0108 starts each member at its channel's last seq. A channel whose person's
// top-level posts and voting agents date from before the upgrade (no wakes marked, every member read to the last seq,
// as 0108 leaves them) wakes no one, by the first pass after start or by the sweep; a post made after it wakes them.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_383_an_upgrade_wakes_no_one_for_posts_made_before_it(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let mut analyst = Agent::new(f, owner.id, "Pricing analyst").await?;
        let mut checker = Agent::new(f, owner.id, "Data checker").await?;
        // Before the upgrade: wakes don't exist yet (held), so the posts are only rows.
        f.gateway.channel_wakes_hold(true);
        let id = channel(f, &cookie, "pricing", &[(&analyst, "member"), (&checker, "member")]).await?;
        post(f, &cookie, &id, text("The Q3 numbers are in; what stands out?"), None).await?;
        post(f, &cookie, &id, text("Anyone?"), None).await?;
        // What 0108 leaves of them: no wakes marked on the old posts, each member at the channel's last seq.
        let channel_id = Uuid::parse_str(&id)?;
        sqlx::query("UPDATE channel_messages SET wakes = '{}', hop = 0 WHERE channel_id = $1")
            .bind(channel_id)
            .execute(&f.pool)
            .await?;
        sqlx::query("UPDATE channel_members m SET read_seq = c.last_seq FROM channels c WHERE c.id = m.channel_id AND c.id = $1")
            .bind(channel_id)
            .execute(&f.pool)
            .await?;
        // The upgraded gateway starts: its first pass and the sweep send nothing.
        f.gateway.channel_wakes_hold(false);
        f.gateway.channel_wakes_now().await?;
        f.gateway.channel_decisions_now().await?;
        let sent: i64 = sqlx::query_scalar("SELECT count(*) FROM channel_wakes WHERE channel_id = $1")
            .bind(channel_id)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(sent, 0, "no wake for posts made before the upgrade");
        analyst.quiet(f).await?;
        checker.quiet(f).await?;
        // A post made after the upgrade wakes both voting agents (a person's post to the channel).
        let asked = post(f, &cookie, &id, text("Here is the file now: which plan lost the most?"), None).await?;
        for agent in [&mut analyst, &mut checker] {
            let sentence = agent.woken(f, &id, 2).await?;
            assert!(sentence.contains(&format!("message {asked}")), "{sentence}");
        }
        Ok(())
    }))
    .await
}
