//! GW-I-165, the run (ADR-0037 §2, §3): the gateway delivers a turn with its sender's keys that the network list allows,
//! each with a new placeholder that no table holds; the run binds them when it starts, they survive a gateway restart,
//! and they go when the run ends.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use support::{TestResult, call};
use tqa_types::{ClientFrame, Event, ServerFrame};

const VALUE: &str = "tqa-test-host-key-run-7c1e";

/// `run.started` or `run.completed` of `run-1`, as the runtime sends it.
fn lifecycle(seq: u64, completed: bool) -> Result<ServerFrame, tqa_types::InvalidId> {
    let run_id: tqa_types::RunId = "run-1".parse()?;
    let event = if completed {
        Event::RunCompleted {
            run_id: run_id.clone(),
        }
    } else {
        Event::RunStarted {
            run_id: run_id.clone(),
        }
    };
    Ok(ServerFrame::Event {
        seq: seq.into(),
        run_id: Some(run_id),
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    })
}

async fn bound(pool: &PgPool) -> Result<Vec<Option<String>>, sqlx::Error> {
    sqlx::query_scalar("SELECT run_id FROM host_key_placeholders ORDER BY created_at")
        .fetch_all(pool)
        .await
}

/// The owner's allowed key `GITHUB_WORK`, a key the list doesn't allow, and another member's key for the same host; a
/// task with its runtime, and the turn delivered. The task, the runtime and the turn's keys.
async fn delivered(
    f: &support::Fixture,
) -> Result<
    (
        tqa_gateway::Conversation,
        support::runtime::Runtime,
        Vec<tqa_types::HostKeyGrant>,
    ),
    Box<dyn std::error::Error>,
> {
    let (owner, cookie) = f.admin().await?;
    let (_, other) = f.member("b@example.invalid").await?;
    let rule = Some(json!({"ports": [443], "version": null}));
    let (status, _) = call(
        f,
        Method::PUT,
        "/api/v1/admin/egress/api.github.com",
        &cookie,
        (rule, Some("rule")),
    )
    .await?;
    assert_eq!(status, 201);
    for (who, name, address, key) in [
        (&cookie, "GITHUB_WORK", "https://api.github.com", "own"),
        (
            &cookie,
            "INTERNAL",
            "https://internal.example.org",
            "unlisted",
        ),
        (&other, "THEIRS", "https://api.github.com", "theirs"),
    ] {
        let body = json!({"name": name, "address": address, "scheme": "bearer", "value": VALUE});
        let (status, added) = call(
            f,
            Method::POST,
            "/api/v1/host-keys",
            who,
            (Some(body), Some(key)),
        )
        .await?;
        assert_eq!(status, 201, "{added}");
    }
    let created = f.conversation(owner.id).await?;
    let mut runtime = f.runtime(&created).await?;
    let path = format!("/api/v1/conversations/{}/messages", created.id);
    f.action(
        &path,
        &cookie,
        Some("message-1"),
        &json!({"text": "push the fix"}),
    )
    .await?
    .error_for_status()?;
    let ClientFrame::UserMessage { host_keys, .. } = runtime.action().await? else {
        return Err("expected the user turn".into());
    };
    Ok((created, runtime, host_keys))
}

// TEST: GW-I-165
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_165_a_turn_carries_only_its_senders_allowed_keys_and_no_table_their_placeholders(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, _runtime, host_keys) = delivered(f).await?;
        let [grant] = host_keys.as_slice() else {
            return Err(format!("one key, not {}", host_keys.len()).into());
        };
        assert_eq!(
            (grant.env(), grant.host(), grant.port(), grant.path_prefix(), grant.user_name()),
            ("GITHUB_WORK", "api.github.com", 443, None, None)
        );
        let placeholder = grant.placeholder().to_owned();
        assert!(placeholder.starts_with("tqa_ph_"));
        // No table holds the placeholder or the value: not the queued turn, not the placeholders' own.
        let tables: String = sqlx::query_scalar(
            "SELECT coalesce((SELECT string_agg(row_to_json(i)::text, '') FROM idempotency i), '') || \
             coalesce((SELECT string_agg(row_to_json(p)::text, '') FROM host_key_placeholders p), '')",
        )
        .fetch_one(&f.pool)
        .await?;
        assert!(!tables.contains(&placeholder[7..]) && !tables.contains(VALUE) && !tables.contains("host_keys"));
        assert_eq!(bound(&f.pool).await?, [None]);
        Ok(())
    })
    .await
}

// TEST: GW-I-165
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_165_the_run_binds_its_keys_a_restart_keeps_them_and_its_end_ends_them(
    pool: PgPool,
) -> TestResult {
    support::run_restarting(pool, async |f| {
        let (created, mut runtime, _) = delivered(f).await?;
        runtime.send(&lifecycle(1, false)?).await?;
        f.until_sequence(&created.id, 1).await?;
        assert_eq!(bound(&f.pool).await?, [Some("run-1".to_owned())]);
        drop(runtime);
        f.restart().await?;
        assert_eq!(bound(&f.pool).await?, [Some("run-1".to_owned())]);
        // The run's end ends it; the row stays a day, so a later use reads expired.
        let mut runtime = f.runtime(&created).await?;
        runtime.send(&lifecycle(2, true)?).await?;
        f.until_sequence(&created.id, 2).await?;
        let ended: Vec<bool> =
            sqlx::query_scalar("SELECT ended_at IS NOT NULL FROM host_key_placeholders")
                .fetch_all(&f.pool)
                .await?;
        // Two: the first placeholder and the one the registration after the restart minted for the run.
        assert_eq!(ended, [true, true]);
        Ok(())
    })
    .await
}

// TEST: GW-I-165
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_165_an_older_runtime_gets_no_keys_and_the_task_says_so(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let rule = Some(json!({"ports": [443], "version": null}));
        let (status, _) = call(f, Method::PUT, "/api/v1/admin/egress/api.github.com", &cookie, (rule, Some("rule"))).await?;
        assert_eq!(status, 201);
        let body = json!({"name": "GITHUB_WORK", "address": "https://api.github.com", "scheme": "bearer", "value": VALUE});
        let (status, _) = call(f, Method::POST, "/api/v1/host-keys", &cookie, (Some(body), Some("own"))).await?;
        assert_eq!(status, 201);

        // A revision-10 runtime still links (the floor is 10), with no install CA in its configuration.
        let created = f.conversation(owner.id).await?;
        let mut runtime = f.runtime_at(&created.id, tqa_types::HOST_KEYS_REVISION - 1).await?;
        assert!(runtime.config.get("host_ca").is_none(), "{}", runtime.config);
        let mut stream = f.stream(&created.id, &cookie, 0).await?;

        // The turn goes out without keys, none are minted, and the task's stream says why in plain words.
        let path = format!("/api/v1/conversations/{}/messages", created.id);
        f.action(&path, &cookie, Some("message-1"), &json!({"text": "push the fix"}))
            .await?
            .error_for_status()?;
        let ClientFrame::UserMessage { host_keys, .. } = runtime.action().await? else {
            return Err("expected the user turn".into());
        };
        assert!(host_keys.is_empty());
        assert_eq!(bound(&f.pool).await?, Vec::<Option<String>>::new());
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                if let (None, ServerFrame::Error { code, message, retryable, target, .. }) = stream.next().await?
                    && code == tqa_types::ErrorCode::HostKeysUnsupported
                {
                    return Ok::<_, Box<dyn std::error::Error>>((message, retryable, target));
                }
            }
        })
        .await??
        .eq(&("This environment is too old to use your keys; rebuild it.".to_owned(), false, None))
        .then_some(())
        .ok_or("the notice says it in plain words")?;
        Ok(())
    })
    .await
}

// TEST: GW-I-165
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_165_a_run_in_progress_gets_its_keys_again_when_its_runtime_registers(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let rule = Some(json!({"ports": [443], "version": null}));
        let (status, _) = call(f, Method::PUT, "/api/v1/admin/egress/api.github.com", &cookie, (rule, Some("rule"))).await?;
        assert_eq!(status, 201);
        let mut second = String::new();
        for name in ["GITHUB_WORK", "SECOND"] {
            let body = json!({"name": name, "address": "https://api.github.com", "scheme": "bearer", "value": VALUE});
            let (status, added) = call(f, Method::POST, "/api/v1/host-keys", &cookie, (Some(body), Some(name))).await?;
            assert_eq!(status, 201, "{added}");
            second = added["id"].as_str().unwrap_or_default().to_owned();
        }
        let created = f.conversation(owner.id).await?;
        let mut runtime = f.runtime(&created).await?;
        assert!(runtime.config.get("run_host_keys").is_none(), "no run is going yet");
        let path = format!("/api/v1/conversations/{}/messages", created.id);
        f.action(&path, &cookie, Some("message-1"), &json!({"text": "push the fix"}))
            .await?
            .error_for_status()?;
        let ClientFrame::UserMessage { host_keys, .. } = runtime.action().await? else {
            return Err("expected the user turn".into());
        };
        assert_eq!(host_keys.len(), 2);
        runtime.send(&lifecycle(1, false)?).await?;
        f.until_sequence(&created.id, 1).await?;

        // The runtime registers again mid-run (it restarted): the run's keys come back with new placeholders, except
        // a key deleted meanwhile; the first placeholders stay bound to the run.
        let (status, _) = call(f, Method::DELETE, &format!("/api/v1/host-keys/{second}?version=1"), &cookie, (None, Some("gone"))).await?;
        assert_eq!(status, 204);
        drop(runtime);
        let mut runtime = f.runtime(&created).await?;
        let again: Vec<tqa_types::HostKeyGrant> = serde_json::from_value(runtime.config["run_host_keys"].clone())?;
        let [grant] = again.as_slice() else {
            return Err(format!("one key again, not {}", again.len()).into());
        };
        assert_eq!(grant.env(), "GITHUB_WORK");
        assert!(host_keys.iter().all(|first| first.placeholder() != grant.placeholder()));
        let runs = bound(&f.pool).await?;
        assert!(runs.len() == 3 && runs.iter().all(|run| run.as_deref() == Some("run-1")), "{runs:?}");

        // Once the run has ended there is nothing to give back.
        runtime.send(&lifecycle(2, true)?).await?;
        f.until_sequence(&created.id, 2).await?;
        drop(runtime);
        let runtime = f.runtime(&created).await?;
        assert!(runtime.config.get("run_host_keys").is_none(), "{}", runtime.config);
        Ok(())
    })
    .await
}
