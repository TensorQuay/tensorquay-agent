//! GW-I-229: the tasks mid-step now, as Settings › Status counts them. A linked runtime's recent run heartbeat and its
//! journal's run still going are both needed: a run that ended since its last beat, a runtime silent for longer than
//! the window, and a running journal with no runtime linked don't count.
use super::{HEARTBEAT_WITHIN, count};
use crate::link::LinkRegistry;
use sqlx::PgPool;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tqa_types::ConversationId;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// `now` less `by`, refusing a clock too young for it rather than panicking.
fn ago(now: tokio::time::Instant, by: Duration) -> Result<tokio::time::Instant, &'static str> {
    now.checked_sub(by)
        .ok_or("the monotonic clock is younger than the test's window")
}

async fn conversation(pool: &PgPool, owner: Uuid) -> Result<Uuid, sqlx::Error> {
    let workspace = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, owner_id, name) VALUES ($1, $2, 'w')")
        .bind(workspace)
        .bind(owner)
        .execute(pool)
        .await?;
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO conversations (id, owner_id, title, model_route, backend, template, workspace_id) \
         VALUES ($1, $2, 'task', 'local', 'docker', 'workstation', $3)",
    )
    .bind(id)
    .bind(owner)
    .bind(workspace)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn person(pool: &PgPool) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, password_hash, role) VALUES ($1, $2::text::citext, 'x', 'member')")
        .bind(id)
        .bind(format!("{id}@example.invalid"))
        .execute(pool)
        .await?;
    Ok(id)
}

/// A lifecycle event as the event store keeps it: the typed `lifecycle` column the status query reads.
async fn lifecycle(
    pool: &PgPool,
    conversation: Uuid,
    seq: i64,
    kind: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO events (conversation_id, seq, ts, frame, lifecycle) \
         VALUES ($1, $2, CURRENT_TIMESTAMP, '{}'::jsonb, $3)",
    )
    .bind(conversation)
    .bind(seq)
    .bind(kind)
    .execute(pool)
    .await?;
    Ok(())
}

// TEST: GW-I-229
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_229_a_task_is_mid_step_while_its_runtime_beats_and_its_run_goes_on(
    pool: PgPool,
) -> TestResult {
    let owner = person(&pool).await?;
    let links = LinkRegistry::new(8, 1, CancellationToken::new());
    let now = tokio::time::Instant::now();
    assert_eq!(count(&pool, &links, now).await?, 0, "nothing linked");

    // Running, linked, a fresh beat: counts.
    let busy = conversation(&pool, owner).await?;
    lifecycle(&pool, busy, 1, "run.started").await?;
    // A resumed run counts the same.
    let resumed = conversation(&pool, owner).await?;
    lifecycle(&pool, resumed, 1, "run.started").await?;
    lifecycle(&pool, resumed, 2, "run.resumed").await?;
    // Its run completed since its last beat: doesn't count.
    let done = conversation(&pool, owner).await?;
    lifecycle(&pool, done, 1, "run.started").await?;
    lifecycle(&pool, done, 2, "run.completed").await?;
    // Running in the journal, but its runtime last beat longer ago than the window: doesn't count.
    let silent = conversation(&pool, owner).await?;
    lifecycle(&pool, silent, 1, "run.started").await?;
    // Running in the journal with no runtime linked (a crash): doesn't count.
    let orphan = conversation(&pool, owner).await?;
    lifecycle(&pool, orphan, 1, "run.started").await?;

    let mut held = Vec::new();
    for (id, beat) in [
        (busy, now),
        (resumed, ago(now, Duration::from_secs(70))?),
        (done, now),
        (silent, ago(now, HEARTBEAT_WITHIN + Duration::from_secs(1))?),
    ] {
        let conversation: ConversationId = id.to_string().parse()?;
        let link = links.register(conversation.clone(), format!("sandbox-{id}").parse()?)?;
        links.saw_heartbeat(&conversation, link.generation, "run-1".parse()?, beat)?;
        held.push(link);
    }
    assert_eq!(
        count(&pool, &links, now).await?,
        2,
        "the busy and the resumed tasks"
    );
    Ok(())
}

// TEST: GW-I-229
#[test]
fn gw_i_229_the_window_takes_a_late_beat_but_not_a_stopped_run() -> TestResult {
    let links = LinkRegistry::new(4, 1, CancellationToken::new());
    let now = tokio::time::Instant::now();
    let on_time: ConversationId = Uuid::now_v7().to_string().parse()?;
    let late: ConversationId = Uuid::now_v7().to_string().parse()?;
    let never: ConversationId = Uuid::now_v7().to_string().parse()?;
    let mut held = Vec::new();
    for (conversation, beat) in [
        (&on_time, Some(ago(now, Duration::from_secs(30))?)),
        (
            &late,
            Some(ago(now, HEARTBEAT_WITHIN + Duration::from_millis(1))?),
        ),
        (&never, None),
    ] {
        let link = links.register(
            conversation.clone(),
            format!("sandbox-{conversation}").parse()?,
        )?;
        if let Some(beat) = beat {
            links.saw_heartbeat(conversation, link.generation, "run-1".parse()?, beat)?;
        }
        held.push(link);
    }
    assert_eq!(links.heartbeating(now, HEARTBEAT_WITHIN)?, vec![on_time]);
    Ok(())
}
