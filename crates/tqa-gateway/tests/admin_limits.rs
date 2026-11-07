//! GW-I-062 (ADR-0028 slice 5): the admission and sandbox limits are overridable through `/api/v1/admin/limits`. An
//! override applies from the next admission or the next sandbox, never to a running one; writes are versioned,
//! bounded (with usable floors for administrators), ordered, audited and replayable; a reset restores the file's
//! value; the file's values seed the limits under the seed-change rule, and a file edit that leaves a value out of
//! bounds clamps and marks it.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const MIB: u64 = 1024 * 1024;

fn detail(body: &Value) -> (Value, Value) {
    let detail = &body["error"]["details"][0];
    (detail["field"].clone(), detail["reason"].clone())
}

/// The listed limit named `key`.
async fn limit(f: &Fixture, cookie: &str, key: &str) -> Result<Value> {
    let (status, body) = call(f, Method::GET, "/api/v1/admin/limits", cookie, (None, None)).await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["key"] == key)
        .cloned()
        .ok_or("listed")?)
}

async fn set(
    f: &Fixture,
    cookie: &str,
    key: &str,
    value: Value,
    version: i64,
) -> Result<(u16, Value)> {
    call(
        f,
        Method::PUT,
        &format!("/api/v1/admin/limits/{key}"),
        cookie,
        (Some(json!({"value": value, "version": version})), None),
    )
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_062_limits_are_listed_versioned_bounded_and_reset(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[sandbox]\nmax_live=8\npids=512\n[admission]\nslots=6\nper_user=3", async |f| {
        let (admin, cookie) = f.admin().await?;
        let (_, list) = call(f, Method::GET, "/api/v1/admin/limits", &cookie, (None, None)).await?;
        let keys: Vec<&str> = list["items"].as_array().ok_or("items")?.iter().filter_map(|item| item["key"].as_str()).collect();
        // The admission, sandbox and connector limits, the Desktop's two (ADR-0039, GW-U-146) and the computer tool's
        // two caps (GW-U-152).
        assert_eq!(keys.len(), 9);
        assert!(!keys.contains(&"sandbox.park_after_seconds"), "removed by ADR-0035");
        assert!(!keys.contains(&"sandbox.pids"), "an environment's size since the environment sizes design");
        let rate = limit(f, &cookie, "mcp.calls_per_minute").await?;
        assert_eq!(
            (&rate["value"], &rate["default"], &rate["min"], &rate["max"], &rate["unit"], &rate["applies"]),
            (&json!(60), &json!(60), &json!(1), &json!(6000), &json!("calls"), &json!("next_call"))
        );
        for (key, value, max, unit, applies) in [
            ("desktop.max_sessions", 2, 6, "sandboxes", "next_admission"),
            ("desktop.screenshot_retention_days", 30, 365, "days", "next_purge"),
            ("computer.max_screenshots_per_run", 200, 1000, "screenshots", "next_session"),
            ("computer.max_actions_per_run", 1000, 5000, "actions", "next_session"),
        ] {
            let item = limit(f, &cookie, key).await?;
            assert_eq!(
                (&item["value"], &item["default"], &item["min"], &item["max"], &item["unit"], &item["applies"]),
                (&json!(value), &json!(value), &json!(1), &json!(max), &json!(unit), &json!(applies)),
                "{key}"
            );
        }
        let slots = limit(f, &cookie, "admission.slots").await?;
        assert_eq!(
            (&slots["value"], &slots["default"], &slots["min"], &slots["max"], &slots["unit"], &slots["applies"], &slots["seed"], &slots["last_set_by"]),
            (&json!(6), &json!(6), &json!(1), &json!(8), &json!("sandboxes"), &json!("next_admission"), &json!("current"), &json!("seed"))
        );
        let (status, changed) = set(f, &cookie, "admission.per_user", json!(5), 1).await?;
        assert_eq!((status, &changed["value"], &changed["version"], &changed["last_set_by"]), (200, &json!(5), &json!(2), &json!("admin")));
        let (status, stale) = set(f, &cookie, "admission.per_user", json!(4), 1).await?;
        assert_eq!((status, &stale["error"]["code"]), (409, &json!("conflict")));
        let refusals = [
            ("admission.per_user", json!(7), 2, ("value", "limit_order")),
            ("admission.slots", json!(4), 1, ("limits", "limit_order")),
            ("admission.slots", json!(9), 1, ("value", "limit_out_of_range")),
            ("sandbox.pids", json!(512), 1, ("key", "limit_unknown")),
            ("sandbox.memory_bytes", json!(512 * MIB), 1, ("key", "limit_unknown")),
            ("sandbox.cpu_millicores", json!(1000), 1, ("key", "limit_unknown")),
            ("sandbox.idle_seconds", json!(-1), 1, ("value", "limit_out_of_range")),
            ("sandbox.idle_seconds", json!("60"), 1, ("value", "limit_out_of_range")),
            ("sandbox.max_live", json!(4), 1, ("key", "limit_unknown")),
            ("sandbox.park_after_seconds", json!(60), 1, ("key", "limit_unknown")),
        ];
        for (key, value, version, (field, reason)) in refusals {
            let (status, body) = set(f, &cookie, key, value.clone(), version).await?;
            assert_eq!((status, detail(&body)), (400, (json!(field), json!(reason))), "{key} {value}");
        }
        let (status, reset) = call(f, Method::DELETE, "/api/v1/admin/limits/admission.per_user?version=2", &cookie, (None, None)).await?;
        assert_eq!((status, &reset["value"], &reset["version"], &reset["last_set_by"]), (200, &json!(3), &json!(3), &json!("seed")));
        let audit: Vec<(String, Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT action,before_version,after_version FROM admin_audit WHERE area='limits' AND actor_user_id=$1 AND item='admission.per_user' ORDER BY at, id",
        )
        .bind(admin.id)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(audit, vec![("replace".into(), Some(1), Some(2)), ("replace".into(), Some(2), Some(3))]);
        Ok(())
    })
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_062_retries_replay_and_only_administrators_write(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        let path = "/api/v1/admin/limits/sandbox.idle_seconds";
        let body = || Some(json!({"value": 300, "version": 1}));
        let (status, first) = call(f, Method::PUT, path, &cookie, (body(), Some("idle-1"))).await?;
        assert_eq!(status, 200, "{first}");
        let (status, replay) =
            call(f, Method::PUT, path, &cookie, (body(), Some("idle-1"))).await?;
        assert_eq!(
            (status, &replay),
            (200, &first),
            "a retry replays instead of answering 409"
        );
        assert_eq!(
            call(
                f,
                Method::GET,
                "/api/v1/admin/limits",
                &member,
                (None, None)
            )
            .await?
            .0,
            403
        );
        assert_eq!(
            call(
                f,
                Method::PUT,
                path,
                &member,
                (Some(json!({"value": 300, "version": 2})), None)
            )
            .await?
            .0,
            403
        );
        let (_, token) = call(
            f,
            Method::POST,
            "/api/v1/tokens",
            &cookie,
            (Some(json!({"name":"admin","expires_in_days":1})), None),
        )
        .await?;
        let response = f
            .client
            .put(f.url(path))
            .bearer_auth(token["token"].as_str().unwrap_or_default())
            .json(&json!({"value": 400, "version": 2}))
            .send()
            .await?;
        assert_eq!(
            response.status(),
            200,
            "an administrator's API token may change limits"
        );
        let (status, body) = call(
            f,
            Method::PUT,
            path,
            &cookie,
            (Some(json!({"value": 400})), None),
        )
        .await?;
        assert_eq!(
            (status, detail(&body)),
            (400, (json!("body"), json!("body_invalid"))),
            "a version is required"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_062_an_override_applies_to_the_next_sandbox_never_a_running_one(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let first = f.conversation(admin.id).await?;
        let _running = f.runtime(&first).await?;
        let before = f.backend.limits(&first.id).ok_or("first spec")?;
        let timers = |id: uuid::Uuid| {
            sqlx::query_as::<_, (Option<i32>, Option<i32>)>(
                "SELECT idle_seconds,max_lifetime_seconds FROM sandbox_instances WHERE conversation_id=$1",
            )
            .bind(id)
        };
        let first_id = uuid::Uuid::parse_str(first.id.as_str())?;
        let admitted = timers(first_id).fetch_one(&f.pool).await?;
        // Sizes are an environment's since the environment sizes design (GW-I-137); the timers are still Limits'.
        for (key, value) in [("sandbox.idle_seconds", json!(4321))] {
            let version = limit(f, &cookie, key).await?["version"].as_i64().ok_or("version")?;
            assert_eq!(set(f, &cookie, key, value, version).await?.0, 200, "{key}");
        }
        assert_eq!(f.backend.limits(&first.id), Some(before), "the running sandbox keeps its limits");
        assert_eq!(timers(first_id).fetch_one(&f.pool).await?, admitted, "and the timers it was admitted with");
        let second = f.conversation(admin.id).await?;
        let _next = f.runtime(&second).await?;
        assert_eq!(f.backend.limits(&second.id), Some(before), "Limits no longer size a sandbox");
        let second_id = uuid::Uuid::parse_str(second.id.as_str())?;
        assert_eq!(timers(second_id).fetch_one(&f.pool).await?.0, Some(4321));
        Ok(())
    })
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_062_a_lowered_admission_limit_holds_the_next_sandbox_back(
    pool: PgPool,
) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[sandbox]\nmax_live=4\n[admission]\nslots=4\nper_user=4",
        async |f| {
            let (admin, cookie) = f.admin().await?;
            let first = f.conversation(admin.id).await?;
            let _running = f.runtime(&first).await?;
            assert_eq!(
                set(f, &cookie, "admission.per_user", json!(1), 1).await?.0,
                200
            );
            let second = f.conversation(admin.id).await?;
            let refused = f.gateway.sandboxes().ensure(&second.id).await;
            assert!(
                refused.is_err(),
                "the owner is at the new per-user cap: {refused:?}"
            );
            assert_eq!(
                f.backend.destroys.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "the running sandbox is left alone"
            );
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_062_the_file_seeds_the_limits_and_a_file_edit_clamps_what_no_longer_fits(
    pool: PgPool,
) -> TestResult {
    let file = |max_live: u32, idle: u32| {
        format!(
            "[sandbox]\nmax_live={max_live}\nidle_seconds={idle}\n[admission]\nslots=2\nper_user=2"
        )
    };
    support::run_restarting_with(pool, None, &file(8, 600), async |f| {
        let (_, cookie) = f.admin().await?;
        assert_eq!(set(f, &cookie, "admission.slots", json!(7), 1).await?.0, 200);
        assert_eq!(set(f, &cookie, "sandbox.idle_seconds", json!(300), 1).await?.0, 200);
        f.restart().await?;
        let idle = limit(f, &cookie, "sandbox.idle_seconds").await?;
        assert_eq!((&idle["value"], &idle["last_set_by"]), (&json!(300), &json!("admin")), "an unchanged file keeps it");
        f.reconfigure(&file(8, 900))?;
        f.restart().await?;
        let idle = limit(f, &cookie, "sandbox.idle_seconds").await?;
        assert_eq!((&idle["value"], &idle["last_set_by"]), (&json!(900), &json!("seed")), "an edited file wins");
        // The file's own slots (2) are unchanged, so the administrator's 7 stands until the file lowers max_live to 4 below
        // it: then it is clamped, marked and audited as the seed.
        f.reconfigure(&file(4, 900))?;
        f.restart().await?;
        let slots = limit(f, &cookie, "admission.slots").await?;
        assert_eq!((&slots["value"], &slots["clamped_from"], &slots["max"]), (&json!(4), &json!(7), &json!(4)));
        let seeded: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM admin_audit WHERE area='limits' AND item='admission.slots' AND actor_kind='seed' AND action='replace'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert!(seeded >= 1, "the clamp is audited as a seed change");
        Ok(())
    })
    .await
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_062_a_sandbox_admitted_before_the_upgrade_still_idles(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[sandbox]\nidle_seconds=1", async |f| {
        let (admin, _) = f.admin().await?;
        let conversation = f.conversation(admin.id).await?;
        let _runtime = f.runtime(&conversation).await?;
        // As a row from before migration 0037: no captured timers, so it follows the current settings.
        sqlx::query("UPDATE sandbox_instances SET idle_seconds=NULL,max_lifetime_seconds=NULL")
            .execute(&f.pool)
            .await?;
        tokio::time::timeout(support::DEADLINE, async {
            loop {
                let records: i64 = sqlx::query_scalar("SELECT count(*) FROM sandbox_instances")
                    .fetch_one(&f.pool)
                    .await?;
                if records == 0 {
                    return Ok::<(), sqlx::Error>(());
                }
                // Real time: the idle timer is one real second, as in GW-I-010.
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await??;
        Ok(())
    })
    .await
}

// TEST: GW-I-203
/// ADR-0034 S6: the agent teams' limits are administrator overrides while agent teams are on, and leave no trace while
/// they are off: not listed, and a write is `403 teams_disabled`.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_203_team_limits_are_hidden_and_refused_while_agent_teams_are_off(
    pool: PgPool,
) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[sandbox]\nmax_live=8\n[admission]\nslots=6\nper_user=3",
        async |f| {
            let (_, cookie) = f.admin().await?;
            let (_, list) = call(
                f,
                Method::GET,
                "/api/v1/admin/limits",
                &cookie,
                (None, None),
            )
            .await?;
            let keys: Vec<&str> = list["items"]
                .as_array()
                .ok_or("items")?
                .iter()
                .filter_map(|item| item["key"].as_str())
                .collect();
            assert!(
                keys.iter().all(|key| !key.starts_with("agent_teams.")),
                "{keys:?}"
            );
            for key in ["agent_teams.max_members", "agent_teams.token_budget"] {
                let (status, body) = set(f, &cookie, key, json!(2), 1).await?;
                assert_eq!(
                    (status, &body["error"]["code"]),
                    (403, &json!("teams_disabled")),
                    "{key}"
                );
                let (status, body) = call(
                    f,
                    Method::DELETE,
                    &format!("/api/v1/admin/limits/{key}?version=1"),
                    &cookie,
                    (None, None),
                )
                .await?;
                assert_eq!(
                    (status, &body["error"]["code"]),
                    (403, &json!("teams_disabled")),
                    "{key}"
                );
            }
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-203
/// While agent teams are on, the six team limits are listed with their units, the bounds an administrator may set and
/// when each applies; the helper cap is refused above what the admission limits leave beside the lead.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_203_team_limits_are_listed_bounded_and_ordered_while_agent_teams_are_on(
    pool: PgPool,
) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[sandbox]\nmax_live=8\n[admission]\nslots=6\nper_user=3\n[agent_teams]\nenabled=true",
        async |f| {
            let (_, cookie) = f.admin().await?;
            let expected = [
                ("agent_teams.max_members", 3, 1, 2, "helpers", "next_member"),
                (
                    "agent_teams.token_budget",
                    2_000_000,
                    10_000,
                    100_000_000,
                    "tokens",
                    "next_team",
                ),
                (
                    "agent_teams.deadline_minutes",
                    120,
                    5,
                    1_440,
                    "minutes",
                    "next_team",
                ),
                (
                    "agent_teams.member_idle_minutes",
                    10,
                    1,
                    1_440,
                    "minutes",
                    "next_check",
                ),
                (
                    "agent_teams.member_stuck_minutes",
                    15,
                    1,
                    1_440,
                    "minutes",
                    "next_check",
                ),
                (
                    "agent_teams.idle_stop_minutes",
                    30,
                    1,
                    1_440,
                    "minutes",
                    "next_check",
                ),
            ];
            for (key, value, min, max, unit, applies) in expected {
                let item = limit(f, &cookie, key).await?;
                assert_eq!(
                    (
                        &item["value"],
                        &item["min"],
                        &item["max"],
                        &item["unit"],
                        &item["applies"]
                    ),
                    (
                        &json!(value),
                        &json!(min),
                        &json!(max),
                        &json!(unit),
                        &json!(applies)
                    ),
                    "{key}"
                );
            }
            // A per-user cap of 3 leaves room for 2 helpers beside the lead.
            let (status, body) = set(f, &cookie, "agent_teams.max_members", json!(3), 1).await?;
            assert_eq!(
                (status, detail(&body)),
                (400, (json!("value"), json!("limit_order")))
            );
            let (status, body) = set(f, &cookie, "agent_teams.max_members", json!(2), 1).await?;
            assert_eq!((status, &body["value"]), (200, &json!(2)), "{body}");
            for (key, value) in [
                ("agent_teams.token_budget", 9_999),
                ("agent_teams.deadline_minutes", 4),
            ] {
                let (status, body) = set(f, &cookie, key, json!(value), 1).await?;
                assert_eq!(
                    (status, detail(&body)),
                    (400, (json!("value"), json!("limit_out_of_range"))),
                    "{key}"
                );
            }
            // Lowering the admission limits is never refused for the helper cap: the effective cap drops instead.
            let (status, body) = set(f, &cookie, "admission.per_user", json!(2), 1).await?;
            assert_eq!(status, 200, "{body}");
            Ok(())
        },
    )
    .await
}
