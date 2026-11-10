//! GW-I-254 (the Product Owner's edge cases, 7 Oct 2026): a helper whose sandbox keeps stopping mid-run. Each loss is
//! replaced and the run resumes, as for any task (GW-I-114); the fifth in ten minutes retires the sandbox as failed
//! with the run unfinished. For a helper that run still reads as going, so the lead heard nothing until it looked stuck
//! 15 minutes later. Now the lead is told at once that the helper failed, in words, and once.
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use std::{sync::atomic::Ordering, time::Duration};
use support::TestResult;
use tqa_sandbox::SandboxBackend;
use tqa_types::ConversationId;
use uuid::Uuid;

/// `id`'s live sandbox.
async fn sandbox_of(
    f: &support::Fixture,
    id: &ConversationId,
) -> Result<tqa_sandbox::SandboxHandle> {
    Ok(f.backend
        .list()
        .await?
        .into_iter()
        .find(|handle| handle.conversation_id == *id)
        .ok_or("the helper's sandbox")?)
}

/// The team's notices to the lead: kind, member and text.
async fn notices(f: &support::Fixture, team: Uuid) -> Result<Vec<(String, String, String)>> {
    Ok(sqlx::query_as(
        "SELECT m.body->>'kind', m.body->>'member', m.body->>'text' FROM channel_messages m \
         JOIN agent_teams t ON t.channel_id = m.channel_id WHERE t.id = $1 AND m.type = 'notice' ORDER BY m.seq",
    )
    .bind(team)
    .fetch_all(&f.pool)
    .await?)
}

/// Waits until the backend has created more than `before` sandboxes, advancing the frozen clock by `delay` each time.
async fn created_after(f: &support::Fixture, before: usize, delay: u64) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        while f.backend.creates.load(Ordering::SeqCst) <= before {
            if delay > 0 {
                f.clock.advance(Duration::from_secs(delay));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| format!("no sandbox created (delay {delay} s)"))?;
    Ok(())
}

/// Waits until `id` has no sandbox record.
async fn retired(f: &support::Fixture, conversation: Uuid) -> TestResult {
    tokio::time::timeout(support::DEADLINE, async {
        loop {
            let records: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM sandbox_instances WHERE conversation_id = $1",
            )
            .bind(conversation)
            .fetch_one(&f.pool)
            .await?;
            if records == 0 {
                return Ok::<_, Box<dyn std::error::Error>>(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .map_err(|_| "the fifth loss didn't retire the sandbox")??;
    Ok(())
}

/// What follows the fifth loss.
#[derive(Clone, Copy, PartialEq)]
enum Then {
    /// The sandbox is retired as failed, its run unfinished.
    Retired,
    /// A turn still queued for the task starts a new sandbox at once (admission doesn't know about the give-up).
    StartedAgain,
}

/// Loses `id`'s sandbox until the manager gives up on it: four losses, each replaced (after 0, 5, 15 and 45 s) with a
/// new runtime, then the fifth in the window, which gives up. `runtime` is the live sandbox's runtime, held only to
/// keep its link open.
async fn give_up(
    f: &support::Fixture,
    id: &ConversationId,
    runtime: support::runtime::Runtime,
    then: Then,
) -> TestResult {
    let conversation = Uuid::parse_str(id.as_str())?;
    let mut runtime = Some(runtime);
    // Its sandbox is lost four times, each replaced (after 0, 5, 15 and 45 s).
    for delay in [0_u64, 5, 15, 45] {
        // Past its start first: a sandbox lost while its worker still waits for it to be ready fails that start
        // instead of counting as a loss (and the clock's reads can come from any of the team's four workers).
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let lost = sandbox_of(f, id).await?;
        let created = f.backend.creates.load(Ordering::SeqCst);
        f.backend.lose(&lost).await?;
        drop(runtime.take());
        f.clock.observed().await?;
        created_after(f, created, delay).await?;
        f.forget_runtime(id)?;
        runtime = Some(f.runtime_for(id).await?);
        f.clock.observed().await?;
    }
    // The fifth loss in the window gives up.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let lost = sandbox_of(f, id).await?;
    let created = f.backend.creates.load(Ordering::SeqCst);
    f.backend.lose(&lost).await?;
    drop(runtime.take());
    f.clock.observed().await?;
    match then {
        Then::Retired => retired(f, conversation).await,
        Then::StartedAgain => created_after(f, created, 0).await,
    }
}

// TEST: GW-I-254
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_254_a_helper_whose_sandbox_keeps_stopping_is_told_to_the_lead_as_failed(
    pool: sqlx::PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let mut t = team(f).await?;
        let id = t.researcher.0.clone();
        let conversation = Uuid::parse_str(id.as_str())?;
        f.clock.freeze();
        f.clock.observed().await?;
        // The researcher's run (r-2) goes on while its sandbox is lost five times: the fifth retires it as failed.
        // The harness keeps the runtimes only to hold their links open; this test takes the researcher's to lose it.
        #[allow(clippy::used_underscore_binding)]
        let runtime = t.runtimes.remove(1);
        give_up(f, &id, runtime, Then::Retired).await?;
        f.gateway.reconcile_agent_teams_now().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        let told = notices(f, t.id).await?;
        assert_eq!(told.len(), 1, "the lead is told once: {told:?}");
        let (kind, member, text) = &told[0];
        assert_eq!(
            (kind.as_str(), member.as_str()),
            ("member_failed", "researcher")
        );
        assert!(
            text.starts_with("researcher failed: its sandbox stopped"),
            "{text}"
        );
        assert!(
            !text.contains("sandbox_failed"),
            "words, never a code: {text}"
        );
        // Failed as a failed run is, so its page says so and the lead's next message starts it (GW-I-260).
        let (state, failed_run): (String, bool) = sqlx::query_as(
            "SELECT state, failed_run FROM agent_team_members WHERE conversation_id = $1",
        )
        .bind(conversation)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!((state.as_str(), failed_run), ("failed", true));
        // Past member_stuck_minutes the given-up run still reads as unfinished: no "seems stuck" after "failed", and
        // the helper stays failed.
        sqlx::query(
            "UPDATE events SET created_at = created_at - make_interval(mins => 16) WHERE conversation_id = $1",
        )
        .bind(conversation)
        .execute(&f.pool)
        .await?;
        f.gateway.reconcile_agent_teams_now().await?;
        f.gateway.reconcile_agent_teams_now().await?;
        let told = notices(f, t.id).await?;
        assert_eq!(told.len(), 1, "one notice past the stuck threshold: {told:?}");
        let state: String =
            sqlx::query_scalar("SELECT state FROM agent_team_members WHERE conversation_id = $1")
                .bind(conversation)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(state, "failed");
        // The lead's next message wakes it, though its cut run has no end stored (backlog B1): the wake is what starts
        // it again, as the notice says.
        let (status, answer) = send(
            f,
            &t.lead.1,
            "r-1:c-9",
            &serde_json::json!({"type": "assign", "to": "researcher", "text": "Try again."}),
        )
        .await?;
        assert_eq!(status, 201, "{answer}");
        assert_eq!(
            wakes_settle(f, &id, 1).await?.len(),
            1,
            "a given-up helper is woken by its lead's message"
        );
        // The wake starts a new sandbox. Its runtime resumes the cut run from its journal (it reads unfinished there
        // too), which makes the helper working again; the wake's turn waits for that run's end and then goes out,
        // once.
        let created = f.backend.creates.load(Ordering::SeqCst);
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.creates.load(Ordering::SeqCst) == created && sandbox_of(f, &id).await.is_err() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| "the wake started no sandbox")?;
        f.forget_runtime(&id)?;
        let mut runtime = f.runtime_for(&id).await?;
        let last: i64 = sqlx::query_scalar("SELECT MAX(seq) FROM events WHERE conversation_id = $1")
            .bind(conversation)
            .fetch_one(&f.pool)
            .await?;
        let next = u64::try_from(last)?;
        let frame = |seq: u64, event: tqa_types::Event| tqa_types::ServerFrame::Event {
            seq: seq.into(),
            run_id: None,
            step: None,
            ts: "2026-01-01T00:00:00Z".into(),
            event,
        };
        runtime
            .send(&frame(
                next + 1,
                tqa_types::Event::RunResumed {
                    run_id: "r-2".parse()?,
                    from_step: 1,
                },
            ))
            .await?;
        f.until_sequence(&id, next + 1).await?;
        let (state, failed_run): (String, bool) = sqlx::query_as(
            "SELECT state, failed_run FROM agent_team_members WHERE conversation_id = $1",
        )
        .bind(conversation)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!((state.as_str(), failed_run), ("working", false));
        runtime
            .send(&frame(
                next + 2,
                tqa_types::Event::RunCompleted {
                    run_id: "r-2".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&id, next + 2).await?;
        // Between the resumed run's end and the wake's turn going out (the same pass or a later one), the lead hears
        // nothing more: still the one notice.
        f.gateway.reconcile_agent_teams_now().await?;
        let told = notices(f, t.id).await?;
        assert_eq!(told.len(), 1, "no idle or done notice after the resumed run: {told:?}");
        assert!(
            matches!(
                runtime.action().await?,
                tqa_types::ClientFrame::UserMessage {
                    team_wake: Some(_),
                    ..
                }
            ),
            "the wake's turn goes out once the resumed run ends"
        );
        assert_eq!(wakes(f, &id).await?.len(), 1, "one wake");
        Ok(())
    })
    .await
}

/// `id`'s count of failures and its team-wake actions with their status, oldest first.
async fn failures_and_wakes(
    f: &support::Fixture,
    id: &ConversationId,
) -> Result<(i32, Vec<(String, String)>)> {
    let conversation = Uuid::parse_str(id.as_str())?;
    let failures: i32 =
        sqlx::query_scalar("SELECT failures FROM agent_team_members WHERE conversation_id = $1")
            .bind(conversation)
            .fetch_one(&f.pool)
            .await?;
    let wakes = sqlx::query_as(
        "SELECT key, execution_status FROM idempotency WHERE conversation_id = $1 AND key LIKE 'team-wake:%' \
         ORDER BY ordinal",
    )
    .bind(conversation)
    .fetch_all(&f.pool)
    .await?;
    Ok((failures, wakes))
}

// TEST: GW-I-254
// Given up, woken, and given up again before the new sandbox's runtime stores any run event: each give-up counts once
// and is told once, and the helper isn't left failed with nothing to start it, since the first wake's turn still waits
// and starts a new sandbox.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_254_a_helper_given_up_twice_counts_both_and_is_started_again(
    pool: sqlx::PgPool,
) -> TestResult {
    support::run_configured(pool, None, ENABLED, async |f| {
        let mut t = team(f).await?;
        let id = t.researcher.0.clone();
        f.clock.freeze();
        f.clock.observed().await?;
        #[allow(clippy::used_underscore_binding)]
        let runtime = t.runtimes.remove(1);
        give_up(f, &id, runtime, Then::Retired).await?;
        let assign = |key: &'static str| {
            serde_json::json!({"type": "assign", "to": "researcher", "text": key})
        };
        let (status, answer) = send(f, &t.lead.1, "r-1:c-9", &assign("Try again.")).await?;
        assert_eq!(status, 201, "{answer}");
        assert_eq!(wakes_settle(f, &id, 1).await?.len(), 1);
        // The wake's new sandbox: its runtime registers and stores nothing before this sandbox is given up too.
        tokio::time::timeout(support::DEADLINE, async {
            while sandbox_of(f, &id).await.is_err() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| "the wake started no sandbox")?;
        f.forget_runtime(&id)?;
        let runtime = f.runtime_for(&id).await?;
        // The first wake's turn still waits behind the cut run (held, no run event stored), so admission starts a new
        // sandbox for it at once: the helper isn't left failed with nothing to start it.
        give_up(f, &id, runtime, Then::StartedAgain).await?;
        let told: Vec<_> = notices(f, t.id)
            .await?
            .into_iter()
            .filter(|(kind, _, _)| kind == "member_failed")
            .collect();
        assert_eq!(told.len(), 2, "one notice per give-up: {told:?}");
        let (failures, wakes) = failures_and_wakes(f, &id).await?;
        assert_eq!(failures, 2, "each give-up counted once");
        assert_eq!(wakes.len(), 1, "the first wake's turn still waits: {wakes:?}");
        assert_eq!(wakes[0].1, "queued", "{wakes:?}");
        assert!(wakes[0].0.ends_with(":failed-1"), "keyed on the first failure: {wakes:?}");
        // The lead's next message adds no wake while that turn waits (two posts make one wake).
        let (status, answer) = send(f, &t.lead.1, "r-1:c-10", &assign("And again.")).await?;
        assert_eq!(status, 201, "{answer}");
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(failures_and_wakes(f, &id).await?.1.len(), 1, "still one wake");
        Ok(())
    })
    .await
}
