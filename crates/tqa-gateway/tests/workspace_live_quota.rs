//! The live workspace quota on Docker (N4): a running workspace past its quota plus the margin has its run cancelled
//! with a typed notice (GW-I-089), a runtime that does not stop loses its sandbox without a replacement (GW-I-090), and
//! the conversation's runtime state past `sandbox.state_bytes` stops the run too (GW-I-091). The files are sparse: they
//! are charged their full length, and cost the test nothing.
#[path = "support/fixture.rs"]
mod support;

use serde_json::json;
use sqlx::PgPool;
use std::{path::Path, sync::atomic::Ordering, time::Duration};
use support::{Fixture, TestResult};
use tqa_types::{ClientFrame, ConversationId, ErrorCode, ServerFrame};

const MIB: u64 = 1 << 20;

/// A sparse file of `bytes` at `path`.
fn sparse(path: &Path, bytes: u64) -> TestResult {
    std::fs::File::create(path)?.set_len(bytes)?;
    Ok(())
}

fn started(run: &str) -> Result<ServerFrame, Box<dyn std::error::Error>> {
    Ok(ServerFrame::Event {
        seq: 1.into(),
        run_id: Some(run.parse()?),
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event: tqa_types::Event::RunStarted {
            run_id: run.parse()?,
        },
    })
}

/// The first `run.cancel` the runtime receives within `wait`, if any.
async fn cancel(
    runtime: &mut support::runtime::Runtime,
    wait: Duration,
) -> Result<Option<ClientFrame>, Box<dyn std::error::Error>> {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match runtime.try_action(left).await? {
            Some(frame @ ClientFrame::RunCancel { .. }) => return Ok(Some(frame)),
            Some(_) => (),
            None => return Ok(None),
        }
    }
}

/// The next unsequenced error notice with `code` on the stream.
async fn notice(
    stream: &mut support::stream::Stream,
    code: ErrorCode,
) -> Result<bool, Box<dyn std::error::Error>> {
    loop {
        if let (
            None,
            ServerFrame::Error {
                code: seen,
                retryable,
                ..
            },
        ) = stream.next().await?
            && seen == code
        {
            return Ok(retryable);
        }
    }
}

/// A conversation with its runtime linked and its run `r-1` started; the stream opened before the run.
async fn running(
    f: &Fixture,
    before: impl FnOnce(&Path, &Path) -> TestResult,
) -> Result<
    (
        ConversationId,
        String,
        support::runtime::Runtime,
        support::stream::Stream,
        std::path::PathBuf,
    ),
    Box<dyn std::error::Error>,
> {
    let (owner, cookie) = f.admin().await?;
    let conversation = f.conversation(owner.id).await?;
    let id = conversation.id.clone();
    let stream = f.stream(&id, &cookie, 0).await?;
    let mut runtime = f.runtime(&conversation).await?;
    let workspace = f.backend.mounted(&id).ok_or("mounted")?;
    let state = f
        .config_storage_root()
        .join("runtime-state")
        .join(id.as_str());
    std::fs::create_dir_all(&state)?;
    before(&workspace, &state)?;
    // The run starts once the files are there: the first measurement is due as soon as a run is active.
    runtime.send(&started("r-1")?).await?;
    f.until_sequence(&id, 1).await?;
    Ok((id, cookie, runtime, stream, workspace))
}

// TEST: GW-I-089
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_089_a_run_past_its_quota_and_margin_is_cancelled_and_its_workspace_kept(
    pool: PgPool,
) -> TestResult {
    // 1 MiB quota: the margin's floor (256 MiB) applies.
    support::run_configured(
        pool,
        None,
        "[workspaces]\nquota_bytes=1048576\n",
        async |f| {
            let (id, cookie, mut runtime, mut stream, workspace) = running(f, |workspace, _| {
                sparse(&workspace.join("big.bin"), 300 * MIB)
            })
            .await?;
            let Some(ClientFrame::RunCancel { run_id }) =
                cancel(&mut runtime, support::DEADLINE).await?
            else {
                return Err("no run.cancel".into());
            };
            assert_eq!(run_id.as_str(), "r-1");
            assert!(
                !notice(&mut stream, ErrorCode::WorkspaceQuota).await?,
                "the stop is not retryable"
            );
            // The workspace is kept as it is.
            assert_eq!(
                std::fs::metadata(workspace.join("big.bin"))?.len(),
                300 * MIB
            );
            // New runs are refused at once: the live size is recorded.
            let refused = f
                .action(
                    &format!("/api/v1/conversations/{id}/messages"),
                    &cookie,
                    Some("after-stop"),
                    &json!({"text":"keep going"}),
                )
                .await?;
            assert_eq!(refused.status(), 409);
            assert_eq!(
                refused.json::<serde_json::Value>().await?["error"]["code"],
                "workspace_quota"
            );
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-089
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_089_over_the_quota_but_within_the_margin_the_run_continues_after_one_notice(
    pool: PgPool,
) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[workspaces]\nquota_bytes=1048576\n",
        async |f| {
            let (_, _, mut runtime, mut stream, _) = running(f, |workspace, _| {
                sparse(&workspace.join("big.bin"), 2 * MIB)
            })
            .await?;
            assert!(
                notice(&mut stream, ErrorCode::WorkspaceQuota).await?,
                "the notice is retryable"
            );
            assert!(
                cancel(&mut runtime, Duration::from_secs(1))
                    .await?
                    .is_none(),
                "the run is not cancelled within the margin"
            );
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-090
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_090_a_runtime_that_does_not_stop_loses_its_sandbox_without_a_replacement(
    pool: PgPool,
) -> TestResult {
    // The backstop waits two I/O timeouts.
    support::run_configured(
        pool,
        None,
        "[workspaces]\nquota_bytes=1048576\n[limits]\nio_timeout=2\n",
        async |f| {
            let (id, _, mut runtime, _stream, workspace) = running(f, |workspace, _| {
                sparse(&workspace.join("big.bin"), 300 * MIB)
            })
            .await?;
            // The runtime receives the cancel and ignores it.
            assert!(cancel(&mut runtime, support::DEADLINE).await?.is_some());
            f.until_released(&id).await?;
            assert_eq!(f.backend.destroys.load(Ordering::SeqCst), 1);
            // No replacement starts, and the workspace is kept.
            tokio::time::sleep(Duration::from_millis(500)).await;
            let live: i64 = sqlx::query_scalar("SELECT count(*) FROM sandbox_instances")
                .fetch_one(&f.pool)
                .await?;
            assert_eq!(live, 0);
            assert!(workspace.join("big.bin").exists());
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-091
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_091_runtime_state_past_its_bound_stops_the_run(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[sandbox]\nstate_bytes=16777216\n", async |f| {
        let (_, _, mut runtime, mut stream, _) =
            running(f, |_, state| sparse(&state.join("filler"), 20 * MIB)).await?;
        assert!(cancel(&mut runtime, support::DEADLINE).await?.is_some());
        assert!(!notice(&mut stream, ErrorCode::StateQuota).await?);
        Ok(())
    })
    .await
}

/// The size recorded for the workspace `id` runs on, if measured.
async fn recorded(
    f: &Fixture,
    id: &ConversationId,
) -> Result<Option<i64>, Box<dyn std::error::Error>> {
    Ok(sqlx::query_scalar(
        "SELECT w.size_bytes FROM workspaces w JOIN conversations c ON c.workspace_id=w.id WHERE c.id=$1",
    )
    .bind(uuid::Uuid::parse_str(id.as_str())?)
    .fetch_one(&f.pool)
    .await?)
}

// TEST: GW-I-200
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_200_a_workspace_is_measured_when_its_run_ends(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        // The first measurement, due at once when the run starts, finds the workspace empty: "0 B" before the run
        // has written anything (the 13h UI review's item 4).
        let (id, _, mut runtime, _, workspace) = running(f, |_, _| Ok(())).await?;
        tokio::time::timeout(support::DEADLINE, async {
            while recorded(f, &id).await?.is_none() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        })
        .await
        .map_err(|_| "the run's first measurement never came")??;
        assert!(recorded(f, &id).await?.unwrap_or_default() < i64::try_from(MIB)?);
        // The run writes its result, then ends, well before the next live check (30 s); the environment is kept.
        sparse(&workspace.join("result.bin"), 5 * MIB)?;
        runtime
            .send(&ServerFrame::Event {
                seq: 2.into(),
                run_id: Some("r-1".parse()?),
                step: None,
                ts: "2026-01-01T00:00:01Z".into(),
                event: tqa_types::Event::RunCompleted {
                    run_id: "r-1".parse()?,
                },
            })
            .await?;
        f.until_sequence(&id, 2).await?;
        // Measured when the run ends: the kept, idle environment's workspace shows what it holds.
        tokio::time::timeout(support::DEADLINE, async {
            while recorded(f, &id).await?.unwrap_or_default() < i64::try_from(5 * MIB)? {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok::<(), Box<dyn std::error::Error>>(())
        })
        .await
        .map_err(|_| "the workspace was not measured when its run ended")??;
        Ok(())
    })
    .await
}
