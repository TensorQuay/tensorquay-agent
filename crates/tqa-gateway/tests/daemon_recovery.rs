//! SBX-D-003: a missing backend instance interrupts a nonterminal run without losing its journal or mounts.
#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use std::sync::atomic::Ordering;
use support::{TestResult, run_restarting};
use tqa_sandbox::SandboxBackend;
use tqa_types::{Event, SandboxState, ServerFrame};

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn sbx_d_003_missing_container_reconciles_once_and_resumes_on_the_same_workspace(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let mut stream = f.stream(id, &cookie, 0).await?;
        runtime.send(&lifecycle(1, Event::RunStarted { run_id: "interrupted".parse()? })).await?;
        assert_eq!(stream.durable().await?.0, 1.into());
        assert_eq!(stream.state().await?, SandboxState::Busy);
        f.clock.observed().await?;
        let original = f.backend.list().await?.pop().ok_or("sandbox missing")?;
        let mounts = f.backend.mounts(id).ok_or("mounts missing")?;
        f.backend.lose(&original).await?;
        drop(runtime);
        for expected in [SandboxState::Destroyed, SandboxState::Creating] {
            let frame = stream.next().await?.1;
            assert!(matches!(frame, ServerFrame::SessionReady { sandbox, .. } if sandbox.state == expected), "expected {expected:?}, received {frame:?}");
        }
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.creates.load(Ordering::SeqCst) < 2 { tokio::task::yield_now().await; }
        }).await?;
        f.forget_runtime(id)?;
        let mut replacement = f.runtime(&conversation).await?;
        assert_ne!(replacement.sandbox.as_str(), original.id);
        assert_eq!(replacement.config["last_stored_seq"], 1);
        assert_eq!(f.backend.mounts(id).ok_or("replacement mounts")?, mounts);
        replacement.send(&lifecycle(2, Event::RunResumed { run_id: "interrupted".parse()?, from_step: 1 })).await?;
        replacement.send(&lifecycle(3, Event::RunCompleted { run_id: "interrupted".parse()? })).await?;
        for sequence in 2..=3 { assert_eq!(stream.durable().await?.0, sequence.into()); }
        f.gateway.sandboxes().reconcile().await?;
        assert_eq!(f.backend.creates.load(Ordering::SeqCst), 2);
        assert_eq!(f.backend.list().await?.len(), 1);
        assert_eq!(f.gateway.events().read(id, 0.into(), 10).await?.len(), 3);
        Ok(())
    }).await
}

#[sqlx::test]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_011_start_up_replaces_only_interrupted_runs_that_lost_their_sandbox_record(
    pool: PgPool,
) -> TestResult {
    run_restarting(pool, async |f| {
        let (user, _) = f.admin().await?;
        let interrupted = f.conversation(user.id).await?;
        let finished = f.conversation(user.id).await?;
        for (conversation, last) in [
            (&interrupted, None),
            (
                &finished,
                Some(Event::RunCompleted {
                    run_id: "reconciled".parse()?,
                }),
            ),
        ] {
            Box::pin(start_run(f, conversation, last)).await?;
        }
        f.gateway.cancel();
        // A failure cleanup already destroyed the sandboxes and forgot their records.
        for handle in f.backend.list().await? {
            f.backend.lose(&handle).await?;
        }
        sqlx::query("DELETE FROM sandbox_instances")
            .execute(&f.pool)
            .await?;
        f.restart().await?;
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.creates.load(Ordering::SeqCst) < 3 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let live = f.backend.list().await?;
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].conversation_id, interrupted.id);
        f.forget_runtime(&interrupted.id)?;
        let replacement = f.runtime(&interrupted).await?;
        assert_eq!(replacement.config["last_stored_seq"], 1);
        assert_eq!(f.backend.creates.load(Ordering::SeqCst), 3);
        Ok(())
    })
    .await
}

fn lifecycle(sequence: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: sequence.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

/// Starts a run on a live sandbox and optionally stores its last lifecycle event.
async fn start_run(
    f: &support::Fixture,
    conversation: &tqa_gateway::Conversation,
    last: Option<Event>,
) -> TestResult {
    let mut runtime = f.runtime(conversation).await?;
    runtime
        .send(&lifecycle(
            1,
            Event::RunStarted {
                run_id: "reconciled".parse()?,
            },
        ))
        .await?;
    let stored = if let Some(event) = last {
        runtime.send(&lifecycle(2, event)).await?;
        2
    } else {
        1
    };
    f.until_sequence(&conversation.id, stored).await
}
