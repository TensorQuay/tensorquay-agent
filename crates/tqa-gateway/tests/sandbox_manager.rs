//! GW-I-006/GW-I-010/GW-I-011/GW-E-019: bounded, durable admission and single-instance lifecycle ownership.
#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::json;
use sqlx::PgPool;
use std::sync::atomic::Ordering;
use support::{DEADLINE, TestResult, run, run_configured};
use tqa_sandbox::SandboxBackend;

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_twenty_concurrent_actions_create_one_sandbox_and_one_token(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let path = format!("/api/v1/conversations/{}/messages", conversation.id);
        let results = futures::future::try_join_all((0..20).map(|index| {
            let cookie = &cookie;
            let path = &path;
            async move {
                fixture
                    .action(
                        path,
                        cookie,
                        Some(&format!("concurrent-{index}")),
                        &json!({"text":format!("message-{index}")}),
                    )
                    .await
            }
        }))
        .await?;
        for response in results {
            assert_eq!(response.status(), 202);
            assert_eq!(
                response
                    .headers()
                    .get("x-tqa-execution-status")
                    .ok_or("execution header")?,
                "queued"
            );
        }
        let mut runtime = fixture.runtime(&conversation).await?;
        // Messages waiting for the sandbox go out together as user turns (GW-I-129): each one once, none lost.
        let mut delivered = Vec::new();
        while delivered.len() < 20 {
            if let tqa_types::ClientFrame::UserMessage { text, .. } = runtime.action().await? {
                delivered.extend(text.split("\n\n").map(str::to_owned));
            }
        }
        let distinct: std::collections::BTreeSet<_> = delivered.iter().collect();
        assert_eq!((delivered.len(), distinct.len()), (20, 20));
        assert_eq!(fixture.backend.creates.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.backend.list().await?.len(), 1);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM runtime_tokens")
            .fetch_one(&fixture.pool)
            .await?;
        assert_eq!(count, 1);
        wait_status(fixture, "concurrent-0", "delivered").await?;
        let replay = fixture
            .action(
                &path,
                &cookie,
                Some("concurrent-0"),
                &json!({"text":"message-0"}),
            )
            .await?;
        assert_eq!(
            replay
                .headers()
                .get("x-tqa-execution-status")
                .ok_or("status")?,
            "delivered"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_006_registration_timeout_persists_failure_and_replays_original_body(
    pool: PgPool,
) -> TestResult {
    run_configured(pool, None, "[sandbox]\nready_seconds=1", async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let mut stream = fixture.stream(&conversation.id, &cookie, 0).await?;
        let path = format!("/api/v1/conversations/{}/messages", conversation.id);
        let body = json!({"text":"queued before registration"});
        let accepted = fixture
            .action(&path, &cookie, Some("timeout"), &body)
            .await?;
        assert_eq!(accepted.status(), 202);
        let original = accepted.bytes().await?;
        let (sequence, frame) = stream.next().await?;
        assert_eq!(sequence, None);
        assert!(matches!(
            frame,
            tqa_types::ServerFrame::Error {
                seq: None,
                code: tqa_types::ErrorCode::SandboxFailed,
                retryable: true,
                ..
            }
        ));
        wait_status(fixture, "timeout", "failed").await?;
        let replay = fixture
            .action(&path, &cookie, Some("timeout"), &body)
            .await?;
        assert_eq!(replay.status(), 202);
        assert_eq!(
            replay
                .headers()
                .get("x-tqa-execution-status")
                .ok_or("execution header")?,
            "failed"
        );
        assert_eq!(replay.bytes().await?, original);
        // The failure is reported before cleanup (so a failing cleanup never hides it): wait for the cleanup.
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let tokens: i64 = sqlx::query_scalar("SELECT count(*) FROM runtime_tokens")
                    .fetch_one(&fixture.pool)
                    .await?;
                if tokens == 0 && fixture.backend.list().await?.is_empty() {
                    return Ok::<(), Box<dyn std::error::Error>>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await??;
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_010_idle_destroy_revokes_token_and_new_action_gets_a_new_instance(
    pool: PgPool,
) -> TestResult {
    run_configured(pool, None, "[sandbox]\nidle_seconds=1", async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let conversation = fixture.conversation(owner.id).await?;
        let _runtime = fixture.runtime(&conversation).await?;
        let token = fixture
            .backend
            .token(&conversation.id)
            .ok_or("missing token")?;
        // Retirement ends when its record is gone; admission answers busy while it is destroying.
        tokio::time::timeout(DEADLINE, async {
            loop {
                let records: i64 = sqlx::query_scalar("SELECT count(*) FROM sandbox_instances")
                    .fetch_one(&fixture.pool)
                    .await?;
                if fixture.backend.destroys.load(Ordering::SeqCst) > 0 && records == 0 {
                    return Ok::<(), sqlx::Error>(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await??;
        assert!(fixture.backend.list().await?.is_empty());
        assert_eq!(
            fixture
                .gateway
                .runtime_tokens()
                .resolve(&token, std::net::Ipv4Addr::LOCALHOST.into())
                .await?,
            None
        );
        let replacement = fixture.gateway.sandboxes().ensure(&conversation.id).await?;
        assert_eq!(fixture.backend.creates.load(Ordering::SeqCst), 2);
        assert_ne!(
            fixture
                .backend
                .token(&conversation.id)
                .ok_or("replacement")?
                .expose_secret(),
            token.expose_secret()
        );
        assert_eq!(replacement.conversation_id, conversation.id);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_e_019_a_full_host_queues_new_conversations_and_allows_reuse(
    pool: PgPool,
) -> TestResult {
    run_configured(pool, None, "[sandbox]\nmax_live=1", async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let first = fixture.conversation(owner.id).await?;
        let second = fixture.conversation(owner.id).await?;
        let mut runtime = fixture.runtime(&first).await?;
        let accepted = fixture
            .action(
                &format!("/api/v1/conversations/{}/messages", first.id),
                &cookie,
                Some("reuse"),
                &json!({"text":"reuse"}),
            )
            .await?;
        assert_eq!(accepted.status(), 202);
        runtime.action().await?;
        // GW-I-034 (task 048): a full host accepts and queues the work instead of refusing it.
        let queued = fixture
            .action(
                &format!("/api/v1/conversations/{}/messages", second.id),
                &cookie,
                Some("capacity"),
                &json!({"text":"busy"}),
            )
            .await?;
        assert_eq!(queued.status(), 202);
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT execution_status FROM idempotency WHERE key='capacity'"
            )
            .fetch_one(&fixture.pool)
            .await?,
            "queued"
        );
        assert!(
            fixture.backend.created(&second.id).is_none(),
            "no sandbox beyond the budget"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-264
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_264_a_message_on_a_full_slot_table_is_queued_not_refused(pool: PgPool) -> TestResult {
    // PERF-020 on c16: admission counted no sandbox, so it admitted the task and committed the message, then the
    // worker's slot table was full and the person got 429 for a message the gateway kept, with a record no worker or
    // admission pass would serve.
    run_configured(pool, None, "[sandbox]\nmax_live=1", async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let task = fixture.conversation(owner.id).await?;
        let other = fixture.conversation(owner.id).await?;
        // A slot that serves no sandbox row, and that the table can't drop: the table is full, the host isn't.
        let hold = fixture.gateway.sandboxes().hold_slot_for_test(&other.id)?;
        let path = format!("/api/v1/conversations/{}/messages", task.id);
        let body = json!({"text": "on a full slot table"});
        for attempt in ["first", "retry"] {
            let accepted = fixture
                .action(&path, &cookie, Some("full-table"), &body)
                .await?;
            assert_eq!(accepted.status(), 202, "{attempt}: queued, not refused");
        }
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT execution_status FROM idempotency WHERE conversation_id=$1")
                .bind(uuid::Uuid::parse_str(&task.id.to_string())?)
                .fetch_all(&fixture.pool)
                .await?;
        assert_eq!(rows, ["queued"], "one stored message, the retry its replay");
        let recorded: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sandbox_instances WHERE conversation_id=$1")
                .bind(uuid::Uuid::parse_str(&task.id.to_string())?)
                .fetch_one(&fixture.pool)
                .await?;
        assert_eq!(
            recorded, 0,
            "no record without a slot: the admission pass can still start it"
        );
        assert!(fixture.backend.created(&task.id).is_none());
        // The slot frees: the admission pass starts the queued work.
        drop(hold);
        tokio::time::timeout(DEADLINE, async {
            while fixture.backend.created(&task.id).is_none() {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await?;
        Ok(())
    })
    .await
}

// TEST: GW-I-265
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_265_a_create_without_a_subnet_gives_back_its_slot_with_its_record(
    pool: PgPool,
) -> TestResult {
    // PERF-020 on c16: a create that found no free subnet deleted its admission record but left its slot in the table
    // while its worker was still on its way out, so admission counted a free place the table didn't have.
    run_configured(pool, None, "[sandbox]\nmax_live=1", async |fixture| {
        let (owner, cookie) = fixture.admin().await?;
        let task = fixture.conversation(owner.id).await?;
        // The task's slot held as its worker holds it until it ends: only the table's own removal frees the place.
        let hold = fixture.gateway.sandboxes().hold_slot_for_test(&task.id)?;
        fixture.backend.pool_exhausted.store(true, Ordering::SeqCst);
        let path = format!("/api/v1/conversations/{}/messages", task.id);
        let accepted = fixture
            .action(
                &path,
                &cookie,
                Some("no-subnet"),
                &json!({"text": "no subnet"}),
            )
            .await?;
        assert_eq!(accepted.status(), 202);
        tokio::time::timeout(DEADLINE, async {
            while fixture.backend.creates.load(Ordering::SeqCst) == 0
                || fixture
                    .gateway
                    .sandboxes()
                    .table_holds_for_test(&task.id, &hold)
            {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .map_err(|_| "the create without a subnet left its slot in the table")?;
        // A subnet is free again: the queued work starts.
        fixture
            .backend
            .pool_exhausted
            .store(false, Ordering::SeqCst);
        drop(hold);
        fixture.gateway.sandboxes().admission_pass_now().await?;
        tokio::time::timeout(DEADLINE, async {
            while fixture.backend.created(&task.id).is_none() {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await?;
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_011_reconciliation_destroys_orphans_and_revokes_their_tokens(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let managed = fixture.conversation(owner.id).await?;
        let managed_handle = fixture.gateway.sandboxes().ensure(&managed.id).await?;
        let other = fixture.conversation(owner.id).await?;
        let orphan = fixture
            .backend
            .orphan(&managed.id, other.id.clone())
            .await?;
        let token = fixture
            .gateway
            .runtime_tokens()
            .mint(
                &other.id,
                &orphan.id.parse()?,
                std::net::Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        fixture.gateway.sandboxes().reconcile().await?;
        assert_eq!(fixture.backend.list().await?, vec![managed_handle]);
        assert_eq!(
            fixture
                .gateway
                .runtime_tokens()
                .resolve(&token, std::net::Ipv4Addr::LOCALHOST.into())
                .await?,
            None
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-191
/// A create the engine finishes after its last attempt gave up leaves a container nobody recorded; the periodic sweep
/// removes it once it is older than the create budget, and never touches a start in progress (SBX-U-014).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_191_the_periodic_sweep_removes_stray_containers_and_never_a_start_in_progress(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let sandboxes = fixture.gateway.sandboxes();
        let managed = fixture.conversation(owner.id).await?;
        let managed_handle = sandboxes.ensure(&managed.id).await?;
        // A late create's container: its conversation has no sandbox record at all.
        let unrecorded = fixture.conversation(owner.id).await?;
        let stray = fixture
            .backend
            .orphan(&managed.id, unrecorded.id.clone())
            .await?;
        let token = fixture
            .gateway
            .runtime_tokens()
            .mint(&unrecorded.id, &stray.id.parse()?, std::net::Ipv4Addr::LOCALHOST.into())
            .await?;
        // A start in progress: its record is 'starting' and names no sandbox yet, but its container already exists.
        let starting = fixture.conversation(owner.id).await?;
        sqlx::query(
            "INSERT INTO sandbox_instances (conversation_id,idle_seconds,max_lifetime_seconds) VALUES ($1,600,28800)",
        )
        .bind(uuid::Uuid::parse_str(starting.id.as_str())?)
        .execute(&fixture.pool)
        .await?;
        let in_progress = fixture
            .backend
            .orphan(&managed.id, starting.id.clone())
            .await?;
        // A live record that names another container: this one is a stray of the same conversation.
        let second = fixture
            .backend
            .orphan(&managed.id, managed.id.clone())
            .await?;
        let mut kept = vec![managed_handle.clone(), in_progress.clone()];
        kept.sort_by(|a, b| a.id.cmp(&b.id));

        // The fixture's backend dates every container at the epoch. Younger than the budget (120 s): nothing is
        // touched, a stray may still be a create in flight.
        let created = std::time::SystemTime::UNIX_EPOCH;
        let young = created + std::time::Duration::from_mins(2);
        assert_eq!(sandboxes.sweep_containers(young).await?.removed, 0);
        assert_eq!(fixture.backend.list().await?.len(), 4);

        let later = created + std::time::Duration::from_secs(121);
        assert_eq!(sandboxes.sweep_containers(later).await?.removed, 2);
        let mut listed = fixture.backend.list().await?;
        listed.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(listed, kept, "the live sandbox and the start in progress stay; {stray:?} and {second:?} go");
        assert_eq!(
            fixture
                .gateway
                .runtime_tokens()
                .resolve(&token, std::net::Ipv4Addr::LOCALHOST.into())
                .await?,
            None,
            "a removed stray's runtime credential is revoked"
        );
        // The start in progress is still spared however old, until its record names it or is gone.
        let much_later = later + std::time::Duration::from_hours(1);
        assert_eq!(sandboxes.sweep_containers(much_later).await?.removed, 0);
        Ok(())
    })
    .await
}

// TEST: GW-I-191
/// One stray the engine fails to remove doesn't stop the sweep: the others go, the failure is counted (and logged), and
/// a later sweep removes it.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_191_a_stray_that_fails_to_go_is_counted_and_the_sweep_goes_on(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (owner, _) = fixture.admin().await?;
        let sandboxes = fixture.gateway.sandboxes();
        let managed = fixture.conversation(owner.id).await?;
        let managed_handle = sandboxes.ensure(&managed.id).await?;
        for _ in 0..2 {
            let unrecorded = fixture.conversation(owner.id).await?;
            fixture.backend.orphan(&managed.id, unrecorded.id).await?;
        }
        // The first removal times out (a loaded engine); the stray after it is still removed.
        fixture.backend.fail_destroys.store(1, Ordering::SeqCst);
        let later = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(121);
        let swept = sandboxes.sweep_containers(later).await?;
        assert_eq!((swept.removed, swept.failed), (1, 1));
        assert_eq!(fixture.backend.list().await?.len(), 2);
        let swept = sandboxes.sweep_containers(later).await?;
        assert_eq!((swept.removed, swept.failed), (1, 0));
        assert_eq!(fixture.backend.list().await?, vec![managed_handle]);
        Ok(())
    })
    .await
}

async fn wait_status(fixture: &support::Fixture, key: &str, status: &str) -> TestResult {
    tokio::time::timeout(DEADLINE, async {
        loop {
            let actual: String =
                sqlx::query_scalar("SELECT execution_status FROM idempotency WHERE key=$1")
                    .bind(key)
                    .fetch_one(&fixture.pool)
                    .await?;
            if actual == status {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await?
}
