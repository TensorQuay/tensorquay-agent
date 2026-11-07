//! GW-I-177: a batch of plain journal frames is one transaction. It is stored whole, in order, a spilled frame
//! included, and reaches clients only after its commit and in sequence order; a gap, a duplicate or an out-of-order
//! frame anywhere in it refuses all of it, with nothing stored or published; a run lifecycle frame is never in a
//! batch, and through the link its run's end still sets the conversation's last activity.
#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use support::{TestResult, frame, run};
use tqa_types::{Event, ServerFrame};

fn frames(range: std::ops::RangeInclusive<u64>, text: &str) -> Vec<ServerFrame> {
    range.map(|seq| frame(seq, text)).collect()
}

fn lifecycle(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-10-01T00:00:00Z".into(),
        event,
    }
}

/// The sequences a client stream receives next, `count` of them.
async fn received(
    stream: &mut support::stream::Stream,
    count: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut seqs = Vec::with_capacity(count);
    while seqs.len() < count {
        if let (Some(seq), _) = stream.next().await? {
            seqs.push(seq.to_string());
        }
    }
    Ok(seqs)
}

fn seqs(range: std::ops::RangeInclusive<u64>) -> Vec<String> {
    range.map(|seq| seq.to_string()).collect()
}

// TEST: GW-I-177
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_177_a_batch_is_stored_whole_in_order_and_published_after_its_commit(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = &created.id;
        let mut stream = fixture.stream(id, &cookie, 0).await?;
        // The third frame is over the inline payload cap: it spills to a blob inside the same transaction.
        let mut batch = frames(1..=5, "token");
        batch[2] = frame(3, &"x".repeat(70_000));
        fixture
            .gateway
            .events()
            .append_batch(id, batch.clone())
            .await?;
        assert_eq!(
            fixture.gateway.events().read(id, 0.into(), 100).await?,
            batch
        );
        assert_eq!(
            received(&mut stream, 5).await?,
            seqs(1..=5),
            "published in sequence order"
        );
        let (spilled, blob_key): (bool, Option<String>) = sqlx::query_as(
            "SELECT frame IS NULL, blob_key FROM events WHERE conversation_id=$1 AND seq=3",
        )
        .bind(uuid::Uuid::parse_str(id.as_str())?)
        .fetch_one(&fixture.pool)
        .await?;
        assert!(spilled);
        assert_eq!(blob_key, Some(format!("{id}/3")));
        Ok(())
    })
    .await
}

// TEST: GW-I-177
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_177_a_gap_duplicate_or_disorder_anywhere_refuses_the_whole_batch(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = &created.id;
        let events = fixture.gateway.events();
        events.append_batch(id, frames(1..=2, "first")).await?;
        let mut stream = fixture.stream(id, &cookie, 2).await?;
        for (label, batch) in [
            (
                "a gap at the end",
                vec![frame(3, "x"), frame(4, "x"), frame(6, "x")],
            ),
            ("a duplicate inside", vec![frame(3, "x"), frame(3, "x")]),
            ("out of order", vec![frame(4, "x"), frame(3, "x")]),
            ("already stored first", vec![frame(2, "x"), frame(3, "x")]),
        ] {
            assert!(events.append_batch(id, batch).await.is_err(), "{label}");
            assert_eq!(
                events.read(id, 0.into(), 100).await?,
                frames(1..=2, "first"),
                "{label}: nothing stored"
            );
        }
        events.append_batch(id, frames(3..=4, "second")).await?;
        // Nothing of the refused batches reached the client: the next frames it gets are 3 and 4 of the good one.
        assert_eq!(received(&mut stream, 2).await?, seqs(3..=4));
        Ok(())
    })
    .await
}

// TEST: GW-I-177
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_177_a_run_lifecycle_frame_is_never_in_a_batch(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = &created.id;
        let run_id: tqa_types::RunId = "r-1".parse()?;
        let batch = vec![frame(1, "x"), lifecycle(2, Event::RunCompleted { run_id })];
        assert!(
            fixture
                .gateway
                .events()
                .append_batch(id, batch)
                .await
                .is_err()
        );
        assert!(
            fixture
                .gateway
                .events()
                .read(id, 0.into(), 100)
                .await?
                .is_empty()
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-177
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_177_through_the_link_a_runs_end_between_deltas_still_sets_the_last_activity(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = &created.id;
        let uuid = uuid::Uuid::parse_str(id.as_str())?;
        sqlx::query("UPDATE conversations SET last_activity_at='2000-01-01Z' WHERE id=$1")
            .bind(uuid)
            .execute(&fixture.pool)
            .await?;
        let run_id: tqa_types::RunId = "r-1".parse()?;
        let mut runtime = fixture.runtime(&created).await?;
        // Sent back to back, so the deltas queue into batches around the lifecycle frames.
        let mut sent = vec![lifecycle(
            1,
            Event::RunStarted {
                run_id: run_id.clone(),
            },
        )];
        sent.extend(frames(2..=40, "token"));
        sent.push(lifecycle(41, Event::RunCompleted { run_id }));
        sent.extend(frames(42..=60, "after"));
        for frame in &sent {
            runtime.send(frame).await?;
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while fixture
            .gateway
            .events()
            .read(id, 0.into(), 100)
            .await?
            .len()
            < sent.len()
        {
            assert!(
                std::time::Instant::now() < deadline,
                "the frames were not all journaled"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(
            fixture.gateway.events().read(id, 0.into(), 100).await?,
            sent
        );
        let last: time::OffsetDateTime =
            sqlx::query_scalar("SELECT last_activity_at FROM conversations WHERE id=$1")
                .bind(uuid)
                .fetch_one(&fixture.pool)
                .await?;
        assert!(last.year() > 2000, "the run's end set the last activity");
        Ok(())
    })
    .await
}
