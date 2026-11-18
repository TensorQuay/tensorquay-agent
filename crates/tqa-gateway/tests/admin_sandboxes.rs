//! The administrator's Sandboxes page (the Sandboxes page design note, §2): the listing of every running sandbox with
//! its owner, environment, state and size and the install's limits; Stop, which goes through the retire path and
//! refuses a working sandbox; the audit row, the counter and the owner's notice; and the privacy rule, that nothing of
//! a task but its id reaches the answer. On the fixture's engine. GW-U-216 to GW-U-220 and SEC-066.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tower::ServiceExt;
use tqa_types::{Event, ServerFrame};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A title no listing may carry: the privacy rule's planted string.
const PLANTED_TITLE: &str = "Quarterly figures for ACME-7731";

fn lifecycle(sequence: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: sequence.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

async fn listing(f: &Fixture, cookie: &str) -> Result<Value> {
    let (status, body) = call(
        f,
        Method::GET,
        "/api/v1/admin/sandboxes",
        cookie,
        (None, None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(body)
}

fn item<'a>(listing: &'a Value, id: &str) -> Result<&'a Value> {
    listing["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["conversation_id"] == id)
        .ok_or_else(|| format!("{id} not listed").into())
}

/// One task's own read, as its owner sees it.
async fn read(f: &Fixture, id: &str, cookie: &str) -> Result<Value> {
    let (status, body) = call(
        f,
        Method::GET,
        &format!("/api/v1/conversations/{id}"),
        cookie,
        (None, None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(body)
}

async fn metrics(f: &Fixture) -> Result<String> {
    let response = f
        .gateway
        .metrics_router()
        .oneshot(
            axum::http::Request::builder()
                .uri("/metrics")
                .body(axum::body::Body::empty())?,
        )
        .await?;
    Ok(String::from_utf8(
        axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await?
            .to_vec(),
    )?)
}

// GW-U-216, GW-U-220 and SEC-066: the listing's states, totals, limits and order, the privacy rule and the member's
// refusal.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn the_listing_names_every_running_sandbox_by_owner_state_and_size_and_nothing_of_its_task(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (admin, admin_cookie) = f.admin().await?;
        let (member, member_cookie) = f.member("ben@example.invalid").await?;
        // The member's task works in a run; the administrator's own is idle. Both titles are planted.
        let working = f
            .gateway
            .create_for_user(member.id, PLANTED_TITLE, "local")
            .await?;
        let idle = f
            .gateway
            .create_for_user(admin.id, "Notes for ACME-7731", "local")
            .await?;
        let mut working_runtime = f.runtime(&working).await?;
        let _idle_runtime = f.runtime(&idle).await?;
        working_runtime
            .send(&lifecycle(
                1,
                Event::RunStarted {
                    run_id: "listing-run".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&working.id, 1).await?;

        // A member gets 403 on both operations.
        let (status, body) = call(
            f,
            Method::GET,
            "/api/v1/admin/sandboxes",
            &member_cookie,
            (None, None),
        )
        .await?;
        assert_eq!(
            (status, body["error"]["code"].as_str()),
            (403, Some("forbidden"))
        );
        let (status, _) = call(
            f,
            Method::POST,
            &format!("/api/v1/admin/sandboxes/{}/stop", idle.id),
            &member_cookie,
            (None, Some("member-stop")),
        )
        .await?;
        assert_eq!(status, 403);

        let listing = listing(f, &admin_cookie).await?;
        assert_eq!(listing["backend"], "docker");
        let limits = &listing["limits"];
        for key in [
            "slots",
            "per_user",
            "desktops",
            "idle_seconds",
            "max_lifetime_seconds",
        ] {
            assert!(limits[key].is_u64(), "limits.{key}: {limits}");
        }
        let totals = &listing["totals"];
        assert_eq!(
            (
                &totals["in_use"],
                &totals["working"],
                &totals["idle"],
                &totals["starting"],
                &totals["stopping"]
            ),
            (&json!(2), &json!(1), &json!(1), &json!(0), &json!(0))
        );
        assert_eq!(
            (&totals["waiting"], &totals["desktops"]),
            (&json!(0), &json!(0))
        );
        assert!(totals["usage"].is_null(), "nothing measured yet: {totals}");
        assert_eq!(listing["next_after"], Value::Null);
        assert_eq!(listing["left_behind"], json!([]));

        let items = listing["items"].as_array().ok_or("items")?;
        assert_eq!(items.len(), 2);
        // The working one first, the people most affected first.
        assert_eq!(items[0]["conversation_id"], working.id.to_string());
        let row = item(&listing, &working.id.to_string())?;
        assert_eq!(
            row["owner"],
            json!({"id": member.id.to_string(), "email": "ben@example.invalid"})
        );
        assert_eq!(row["environment"]["display_name"], "Minimal");
        assert_eq!(row["environment"]["desktop"], false);
        assert_eq!(row["state"], "working");
        assert_eq!(row["stops_at"], Value::Null);
        assert_eq!(row["kept_on"], false);
        assert_eq!(
            (&row["can_stop"], &row["stop_refusal"]),
            (&json!(false), &json!("working"))
        );
        assert!(
            row["size"]["cpu_millicores"]
                .as_u64()
                .is_some_and(|cores| cores > 0),
            "{row}"
        );
        assert!(
            row["size"]["memory_bytes"]
                .as_u64()
                .is_some_and(|bytes| bytes > 0),
            "{row}"
        );
        assert!(
            row["size"]["pids"].as_u64().is_some_and(|pids| pids > 0),
            "{row}"
        );
        assert!(row["usage"].is_null());
        let reserved = &totals["reserved"];
        assert_eq!(
            reserved["cpu_millicores"].as_u64(),
            items
                .iter()
                .map(|item| item["size"]["cpu_millicores"].as_u64())
                .sum::<Option<u64>>()
        );
        assert_eq!(
            reserved["memory_bytes"].as_u64(),
            items
                .iter()
                .map(|item| item["size"]["memory_bytes"].as_u64())
                .sum::<Option<u64>>()
        );

        let row = item(&listing, &idle.id.to_string())?;
        assert_eq!(row["state"], "idle");
        assert_eq!(
            (&row["can_stop"], &row["stop_refusal"]),
            (&json!(true), &Value::Null)
        );
        assert_eq!(row["owner"]["email"], "admin@example.invalid");
        let last = time::OffsetDateTime::parse(
            row["last_activity_at"].as_str().ok_or("last_activity_at")?,
            &time::format_description::well_known::Rfc3339,
        )?;
        let stops = time::OffsetDateTime::parse(
            row["stops_at"].as_str().ok_or("stops_at")?,
            &time::format_description::well_known::Rfc3339,
        )?;
        assert_eq!(
            (stops - last).whole_seconds(),
            i64::try_from(limits["idle_seconds"].as_u64().ok_or("idle")?)?
        );
        assert!(row["started_at"].is_string());

        // The privacy rule: nothing of a task but its id, the administrator's own task included.
        let text = listing.to_string();
        for planted in [
            PLANTED_TITLE,
            "ACME-7731",
            "Notes for",
            "workspace",
            "title",
            "local",
        ] {
            assert!(!text.contains(planted), "{planted} in the listing: {text}");
        }
        for key in [
            "title",
            "workspace_id",
            "workspace",
            "route",
            "profile",
            "message",
            "branch",
        ] {
            assert!(
                items.iter().all(|item| item.get(key).is_none()),
                "{key} on a row"
            );
        }
        Ok(())
    })
    .await
}

// GW-U-217 and GW-U-218: Stop's retire path, the refusals, the replayed key, the audit row, the counter and the owner's
// notice.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn an_administrators_stop_retires_an_idle_sandbox_refuses_a_working_one_and_is_audited(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (admin, admin_cookie) = f.admin().await?;
        let (member, member_cookie) = f.member("ben@example.invalid").await?;
        let idle = f.gateway.create_for_user(member.id, PLANTED_TITLE, "local").await?;
        let working = f.gateway.create_for_user(member.id, "Working one", "local").await?;
        let _idle_runtime = f.runtime(&idle).await?;
        let mut working_runtime = f.runtime(&working).await?;
        working_runtime
            .send(&lifecycle(1, Event::RunStarted { run_id: "stop-run".parse()? }))
            .await?;
        f.until_sequence(&working.id, 1).await?;
        let before = metrics(f).await?;
        assert!(!before.contains("reason=\"administrator\""));

        // A working sandbox is never stopped by an administrator (decision 11); nothing is retired.
        let (status, body) = call(
            f,
            Method::POST,
            &format!("/api/v1/admin/sandboxes/{}/stop", working.id),
            &admin_cookie,
            (None, Some("stop-working")),
        )
        .await?;
        assert_eq!((status, body["error"]["code"].as_str()), (409, Some("working")), "{body}");
        assert_eq!(f.backend.destroys.load(std::sync::atomic::Ordering::SeqCst), 0);

        // An unknown task, and a known task without a running sandbox, are 404.
        let (status, body) = call(
            f,
            Method::POST,
            &format!("/api/v1/admin/sandboxes/{}/stop", uuid::Uuid::now_v7()),
            &admin_cookie,
            (None, Some("stop-unknown")),
        )
        .await?;
        assert_eq!((status, body["error"]["code"].as_str()), (404, Some("not_found")), "{body}");

        // The key is required, as for every action.
        let (status, _) = call(
            f,
            Method::POST,
            &format!("/api/v1/admin/sandboxes/{}/stop", idle.id),
            &admin_cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 400);

        // Before the stop, the task's own read says it has no notice to read (the page then asks for none).
        assert_eq!(read(f, &idle.id.to_string(), &member_cookie).await?["screen_notices"], json!(false));
        // The idle one goes: accepted, retired once, its record released; the same key answers the same.
        let path = format!("/api/v1/admin/sandboxes/{}/stop", idle.id);
        let (status, body) = call(f, Method::POST, &path, &admin_cookie, (None, Some("stop-idle"))).await?;
        assert_eq!((status, &body), (202, &json!({"stopping": true})));
        let (status, body) = call(f, Method::POST, &path, &admin_cookie, (None, Some("stop-idle"))).await?;
        assert_eq!((status, &body), (202, &json!({"stopping": true})));
        f.until_released(&idle.id).await?;
        assert_eq!(f.backend.destroys.load(std::sync::atomic::Ordering::SeqCst), 1);
        let listing = listing(f, &admin_cookie).await?;
        assert!(item(&listing, &idle.id.to_string()).is_err(), "still listed");
        assert_eq!(listing["totals"]["in_use"], 1);
        // Once gone, a stop is 404; the working one is still refused and still there.
        let (status, body) = call(f, Method::POST, &path, &admin_cookie, (None, Some("stop-idle-again"))).await?;
        assert_eq!((status, body["error"]["code"].as_str()), (404, Some("not_found")), "{body}");
        assert_eq!(f.backend.destroys.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Audited: area sandboxes, action stop, the administrator, the task's id and nothing else.
        let rows = sqlx::query!(
            "SELECT actor_kind, actor_user_id, item, action, before_version, after_version, value_hash_changed \
             FROM admin_audit WHERE area='sandboxes' ORDER BY at, id"
        )
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(rows.len(), 1, "one stop, one row");
        assert_eq!(rows[0].actor_kind, "admin");
        assert_eq!(rows[0].actor_user_id, Some(admin.id));
        assert_eq!((rows[0].item.as_str(), rows[0].action.as_str()), (idle.id.to_string().as_str(), "stop"));
        assert_eq!((rows[0].before_version, rows[0].after_version), (None, None));
        let audit_text: String = sqlx::query_scalar(
            "SELECT coalesce(string_agg(row_to_json(a)::text, ''), '') FROM admin_audit a WHERE area='sandboxes'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert!(!audit_text.contains("ACME-7731") && !audit_text.contains("ben@"), "{audit_text}");

        // Counted once, under its own reason.
        let after = metrics(f).await?;
        assert!(
            after.contains("tqa_sandbox_retired_total{reason=\"administrator\"} 1"),
            "{after}"
        );

        // The owner reads a notice in the stopped task, and none in the other one; it carries no title.
        let (status, notices) = call(
            f,
            Method::GET,
            &format!("/api/v1/conversations/{}/screen/notices", idle.id),
            &member_cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200, "{notices}");
        let items = notices["items"].as_array().ok_or("items")?;
        assert_eq!(items.len(), 1, "{notices}");
        assert_eq!(items[0]["kind"], "sandbox.stopped");
        // The owner isn't told who: no `by`, and nothing of the administrator in any answer the owner reads.
        assert!(items[0].get("by").is_none(), "{notices}");
        assert!(items[0]["at"].is_string() && items[0]["after_seq"].is_u64());
        assert!(!notices.to_string().contains("ACME-7731"));
        assert!(!notices.to_string().contains(&admin.email) && !notices.to_string().contains(&admin.id.to_string()), "{notices}");
        let (_, other) = call(
            f,
            Method::GET,
            &format!("/api/v1/conversations/{}/screen/notices", working.id),
            &member_cookie,
            (None, None),
        )
        .await?;
        assert_eq!(other["items"], json!([]));
        // The task's own read says so, and only there: the stopped task has one to read, the other none, and the
        // list's rows don't carry it.
        assert_eq!(read(f, &idle.id.to_string(), &member_cookie).await?["screen_notices"], json!(true));
        assert_eq!(read(f, &working.id.to_string(), &member_cookie).await?["screen_notices"], json!(false));
        let (status, list) = call(f, Method::GET, "/api/v1/conversations", &member_cookie, (None, None)).await?;
        assert_eq!(status, 200, "{list}");
        let rows = list["items"].as_array().ok_or("items")?;
        assert!(!rows.is_empty() && rows.iter().all(|row| row.get("screen_notices").is_none()), "{list}");
        Ok(())
    })
    .await
}

// GW-U-219: the readiness detail and the metrics carry the running count and the verdict's counts.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn the_readiness_detail_and_the_metrics_carry_the_sandbox_counts(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let conversation = f
            .gateway
            .create_for_user(admin.id, "Counted", "local")
            .await?;
        let _runtime = f.runtime(&conversation).await?;
        let detail: Value = f.get("/readyz?detail=1", &cookie).await?.json().await?;
        let sandboxes = &detail["sandboxes"];
        assert_eq!(sandboxes["running"], 1, "{detail}");
        assert!(sandboxes["oldest_seconds"].is_u64());
        assert_eq!(
            sandboxes["unowned"],
            json!({"containers": 0, "networks": 0}),
            "bind mode: no volumes"
        );
        assert_eq!(
            sandboxes["left_behind"],
            json!({"containers": 0, "networks": 0, "oldest_seconds": null, "reason": null})
        );
        assert!(sandboxes["swept_at"].is_string() || sandboxes["swept_at"].is_null());
        let text = metrics(f).await?;
        assert!(
            text.contains("tqa_sandboxes{backend=\"docker\",state=\"idle\"} 1"),
            "{text}"
        );
        assert!(text.contains("tqa_sandbox_oldest_seconds "), "{text}");
        assert!(
            text.contains("tqa_sandbox_unowned{kind=\"containers\"} 0"),
            "{text}"
        );
        assert!(
            text.contains("tqa_sandbox_left_behind{kind=\"networks\"} 0"),
            "{text}"
        );
        Ok(())
    })
    .await
}
