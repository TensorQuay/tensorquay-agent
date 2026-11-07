//! GW-I-178: batches across a gateway that dies. Killed while batch B's transaction is open, nothing of B is stored;
//! the history ends with A, the runtime resends B (it resends from the last stored sequence), and a client replaying
//! from 0 gets A then B, each event once. Killed after B's commit but before its publication, B is stored once: a
//! client that reconnects from its last event gets B from the store, and the runtime's resend of B is refused as
//! already stored, so no event reaches the client twice. And a link with 256 frames held behind a slow commit still
//! answers the runtime's ping once that commit ends: the held frames go in one batch, not 256 commits.
#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use std::time::{Duration, Instant};
use support::{TestResult, frame, run};
use tqa_types::ServerFrame;

fn frames(range: std::ops::RangeInclusive<u64>, text: &str) -> Vec<ServerFrame> {
    range.map(|seq| frame(seq, text)).collect()
}

/// Every sequence a client stream receives until it has `last`, in the order received.
async fn received_through(
    stream: &mut support::stream::Stream,
    last: u64,
) -> Result<Vec<u64>, Box<dyn std::error::Error>> {
    let mut seqs = Vec::new();
    while seqs.last() != Some(&last) {
        if let (Some(seq), _) = stream.next().await? {
            seqs.push(seq.to_string().parse()?);
        }
    }
    Ok(seqs)
}

// TEST: GW-I-178
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_178_killed_during_a_batch_stores_none_of_it_and_the_resend_stores_it_once(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = created.id.clone();
        fixture
            .gateway
            .events()
            .append_batch(&id, frames(1..=3, "a"))
            .await?;

        // Another connection holds the conversation's row, so B's transaction stops inside, at its lock.
        let mut holder = fixture.pool.begin().await?;
        sqlx::query("SELECT 1 FROM conversations WHERE id=$1 FOR UPDATE")
            .bind(uuid::Uuid::parse_str(id.as_str())?)
            .execute(&mut *holder)
            .await?;
        let events = fixture.gateway.events().clone();
        let (batch_id, batch) = (id.clone(), frames(4..=6, "b"));
        let in_flight = tokio::spawn(async move { events.append_batch(&batch_id, batch).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!in_flight.is_finished(), "B is inside its transaction");
        // The gateway dies: its transaction never commits.
        in_flight.abort();
        assert!(in_flight.await.is_err_and(|error| error.is_cancelled()));
        holder.rollback().await?;

        assert_eq!(
            fixture.gateway.events().read(&id, 0.into(), 100).await?,
            frames(1..=3, "a")
        );
        // The runtime resends from the last stored sequence.
        fixture
            .gateway
            .events()
            .append_batch(&id, frames(4..=6, "b"))
            .await?;
        let mut client = fixture.stream(&id, &cookie, 0).await?;
        assert_eq!(received_through(&mut client, 6).await?, [1, 2, 3, 4, 5, 6]);
        Ok(())
    })
    .await
}

// TEST: GW-I-178
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_178_killed_after_a_commit_before_publication_no_event_reaches_a_client_twice(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = created.id.clone();
        let events = fixture.gateway.events();
        events.append_batch(&id, frames(1..=3, "a")).await?;
        let mut client = fixture.stream(&id, &cookie, 0).await?;
        assert_eq!(received_through(&mut client, 3).await?, [1, 2, 3]);

        // B commits, then the gateway dies before publishing it: the rows are there and no client got them.
        let uuid = uuid::Uuid::parse_str(id.as_str())?;
        for frame in frames(4..=6, "b") {
            let seq: i64 = tqa_gateway_seq(&frame)?;
            sqlx::query(
                "INSERT INTO events (conversation_id,seq,run_id,step,ts,frame) VALUES ($1,$2,NULL,NULL,now(),$3)",
            )
            .bind(uuid)
            .bind(seq)
            .bind(serde_json::to_value(&frame)?)
            .execute(&fixture.pool)
            .await?;
        }
        drop(client);

        // The client reconnects from its last event and the runtime resends B, which is refused as already stored.
        let mut client = fixture.stream(&id, &cookie, 3).await?;
        assert!(events.append_batch(&id, frames(4..=6, "b")).await.is_err());
        events.append_batch(&id, frames(7..=8, "c")).await?;
        assert_eq!(received_through(&mut client, 8).await?, [4, 5, 6, 7, 8], "each event once");
        Ok(())
    })
    .await
}

fn tqa_gateway_seq(frame: &ServerFrame) -> Result<i64, Box<dyn std::error::Error>> {
    let ServerFrame::Event { seq, .. } = frame else {
        return Err("not an event".into());
    };
    Ok(seq.to_string().parse()?)
}

// TEST: GW-I-178
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_178_two_hundred_fifty_six_held_frames_go_in_one_batch_and_the_ping_is_answered(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let created = fixture.conversation(owner.id).await?;
        let id = created.id.clone();
        let mut runtime = fixture.runtime(&created).await?;
        runtime.send(&frame(1, "first")).await?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while fixture
            .gateway
            .events()
            .read(&id, 0.into(), 10)
            .await?
            .is_empty()
        {
            assert!(
                Instant::now() < deadline,
                "the first frame was not journaled"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // A commit that stalls 6 s (within one 10 s attempt): frame 2 waits on it, 256 more are held behind it.
        let pool = fixture.pool.clone();
        let (locked, ready) = tokio::sync::oneshot::channel();
        let released = tokio::spawn(async move {
            let mut transaction = pool.begin().await?;
            sqlx::query("LOCK TABLE events IN ACCESS EXCLUSIVE MODE")
                .execute(&mut *transaction)
                .await?;
            let _ = locked.send(());
            tokio::time::sleep(Duration::from_secs(6)).await;
            transaction.commit().await?;
            Ok::<_, sqlx::Error>(Instant::now())
        });
        ready.await?;
        runtime.send(&frame(2, "slow")).await?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        for seq in 3..=258 {
            runtime.send(&frame(seq, "held")).await?;
        }
        // The ping waits behind the full queue; once the stall ends, one batch commit frees it.
        runtime.roundtrip().await?;
        let answered = Instant::now();
        let ended = released.await??;
        assert!(
            answered.duration_since(ended) < Duration::from_secs(5),
            "the pong came {:?} after the stall ended",
            answered.duration_since(ended)
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while fixture
            .gateway
            .events()
            .read(&id, 0.into(), 1000)
            .await?
            .len()
            < 258
        {
            assert!(
                Instant::now() < deadline,
                "the held frames were not all journaled"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let stored = fixture.gateway.events().read(&id, 0.into(), 1000).await?;
        let expected: Vec<ServerFrame> = std::iter::once(frame(1, "first"))
            .chain(std::iter::once(frame(2, "slow")))
            .chain((3..=258).map(|seq| frame(seq, "held")))
            .collect();
        assert_eq!(stored, expected, "each frame once, in the order sent");
        Ok(())
    })
    .await
}
