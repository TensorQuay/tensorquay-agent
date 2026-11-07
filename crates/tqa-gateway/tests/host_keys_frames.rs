//! GW-I-168 (ADR-0037, Amendment 1): a keyed call's live notices on the task's stream carry the run, the step running
//! when the proxy saw the call and the run's sender; `host_key.used` goes once per run and key, `host_key.refused`
//! once per outcome in its minute; the key's name only when the viewer is the run's sender; after the run, no step.
#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use support::TestResult;
use tqa_types::{Event, HostKeyOutcome, ServerFrame};

fn event(seq: u64, step: Option<u32>, event: Event) -> Result<ServerFrame, tqa_types::InvalidId> {
    Ok(ServerFrame::Event {
        seq: seq.into(),
        run_id: Some("run-1".parse()?),
        step,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    })
}

/// The next keyed-call notice on the stream: its type, step, name and outcome.
async fn next_notice(
    stream: &mut support::stream::Stream,
) -> Result<
    (
        &'static str,
        Option<u32>,
        Option<String>,
        Option<HostKeyOutcome>,
    ),
    Box<dyn std::error::Error>,
> {
    tokio::time::timeout(support::DEADLINE, async {
        loop {
            match stream.next().await? {
                (
                    None,
                    ServerFrame::HostKeyUsed {
                        step, name, run_id, ..
                    },
                ) => {
                    assert_eq!(run_id.as_str(), "run-1");
                    return Ok(("used", step, name, None));
                }
                (
                    None,
                    ServerFrame::HostKeyRefused {
                        step,
                        name,
                        outcome,
                        ..
                    },
                ) => {
                    return Ok(("refused", step, name, Some(outcome)));
                }
                _ => {}
            }
        }
    })
    .await?
}

/// A task whose run-1 is on step 3, a stream on it, its runtime (kept), and its owner.
struct Running {
    id: tqa_types::ConversationId,
    owner: uuid::Uuid,
    stream: support::stream::Stream,
    runtime: support::runtime::Runtime,
}

async fn running(f: &support::Fixture) -> Result<Running, Box<dyn std::error::Error>> {
    let (owner, cookie) = f.admin().await?;
    let created = f.conversation(owner.id).await?;
    let mut runtime = f.runtime(&created).await?;
    runtime
        .send(&event(
            1,
            None,
            Event::RunStarted {
                run_id: "run-1".parse()?,
            },
        )?)
        .await?;
    runtime
        .send(&event(
            2,
            Some(3),
            Event::StepStarted {
                step: 3,
                attempt: 1,
            },
        )?)
        .await?;
    f.until_sequence(&created.id, 2).await?;
    let stream = f.stream(&created.id, &cookie, 2).await?;
    Ok(Running {
        id: created.id,
        owner: owner.id,
        stream,
        runtime,
    })
}

const KEY: &str = "GITHUB_WORK";

/// The proxy's notice of a call by `sender`'s run-1 with KEY, with `outcome`.
async fn notify(f: &support::Fixture, task: &Running, sender: uuid::Uuid, outcome: &str) {
    let key = (uuid::Uuid::from_u128(7), KEY);
    f.gateway
        .host_key_notice(
            &task.id,
            ("run-1", sender),
            Some(key),
            ("api.github.com", 443, outcome),
        )
        .await;
}

// TEST: GW-I-168
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_168_a_use_goes_once_and_a_refusal_once_per_outcome_on_the_running_step(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let mut task = running(f).await?;
        let owner = task.owner;
        for outcome in ["ok", "ok", "wrong_host", "wrong_host", "unavailable"] {
            notify(f, &task, owner, outcome).await;
        }
        assert_eq!(
            next_notice(&mut task.stream).await?,
            ("used", Some(3), Some(KEY.to_owned()), None)
        );
        assert_eq!(
            next_notice(&mut task.stream).await?,
            (
                "refused",
                Some(3),
                Some(KEY.to_owned()),
                Some(HostKeyOutcome::WrongHost)
            )
        );
        assert_eq!(
            next_notice(&mut task.stream).await?,
            (
                "refused",
                Some(3),
                Some(KEY.to_owned()),
                Some(HostKeyOutcome::Unavailable)
            )
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-168
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_168_a_runs_key_is_named_only_to_the_runs_sender(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let mut task = running(f).await?;
        // A run another person sent: this viewer isn't its sender, so the key isn't named.
        notify(f, &task, uuid::Uuid::now_v7(), "foreign_run").await;
        assert_eq!(
            next_notice(&mut task.stream).await?,
            ("refused", Some(3), None, Some(HostKeyOutcome::ForeignRun))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-168
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_168_after_its_run_a_refusal_has_no_step(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let mut task = running(f).await?;
        task.runtime
            .send(&event(
                3,
                None,
                Event::RunCompleted {
                    run_id: "run-1".parse()?,
                },
            )?)
            .await?;
        f.until_sequence(&task.id, 3).await?;
        let owner = task.owner;
        notify(f, &task, owner, "expired").await;
        assert_eq!(
            next_notice(&mut task.stream).await?,
            (
                "refused",
                None,
                Some(KEY.to_owned()),
                Some(HostKeyOutcome::Expired)
            )
        );
        Ok(())
    })
    .await
}
