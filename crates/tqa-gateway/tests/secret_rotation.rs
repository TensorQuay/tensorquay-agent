//! GW-I-093 (vault slice 1, D1): a rotation reminder. The admin list shows when each value last changed and whether a
//! reminder is due; only a changed value moves `rotated_at`; a due secret logs a WARN and counts in a gauge; nothing
//! stops working because a reminder is due. The age is set in the store, standing in for a fake clock.
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tower::ServiceExt as _;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// One global capture for the process: the reminder check logs from the gateway's own tasks.
fn capture_logs() -> Result<log_capture::Capture> {
    static CAPTURE: std::sync::OnceLock<Option<log_capture::Capture>> = std::sync::OnceLock::new();
    CAPTURE
        .get_or_init(|| {
            let capture = log_capture::Capture::default();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(capture.clone())
                .with_ansi(false)
                .with_max_level(tracing::Level::WARN)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .ok()
                .map(|()| capture)
        })
        .clone()
        .ok_or_else(|| "another global tracing subscriber was already set".into())
}

fn detail(body: &Value) -> (Value, Value) {
    let detail = &body["error"]["details"][0];
    (detail["field"].clone(), detail["reason"].clone())
}

async fn listed(f: &Fixture, cookie: &str, name: &str) -> Result<Value> {
    let (status, body) = call(
        f,
        Method::GET,
        "/api/v1/admin/secrets",
        cookie,
        (None, None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["ref"] == format!("secrets/{name}"))
        .cloned()
        .ok_or("listed")?)
}

async fn value(
    f: &Fixture,
    cookie: &str,
    value: &str,
    version: Option<i64>,
) -> Result<(u16, Value)> {
    call(
        f,
        Method::PUT,
        "/api/v1/admin/secrets/provider",
        cookie,
        (Some(json!({"value": value, "version": version})), None),
    )
    .await
}

async fn remind(f: &Fixture, cookie: &str, days: Value, version: i64) -> Result<(u16, Value)> {
    call(
        f,
        Method::PUT,
        "/api/v1/admin/secrets/provider/reminder",
        cookie,
        (
            Some(json!({"rotate_after_days": days, "version": version})),
            None,
        ),
    )
    .await
}

async fn gauge(f: &Fixture) -> Result<String> {
    let response = f
        .gateway
        .metrics_router()
        .oneshot(axum::http::Request::get("/metrics").body(axum::body::Body::empty())?)
        .await?;
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await?;
    Ok(String::from_utf8(bytes.to_vec())?
        .lines()
        .find(|line| line.starts_with("tqa_secrets_rotation_due "))
        .unwrap_or_default()
        .to_owned())
}

async fn age(f: &Fixture, days: i32) -> TestResult {
    sqlx::query("UPDATE secrets SET rotated_at = CURRENT_TIMESTAMP - make_interval(days => $1) WHERE ref='secrets/provider'")
        .bind(days)
        .execute(&f.pool)
        .await?;
    Ok(())
}

// TEST: GW-I-093
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_093_a_rotation_reminder_is_shown_logged_and_counted_but_never_blocks(
    pool: PgPool,
) -> TestResult {
    let logs = capture_logs()?;
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        assert_eq!(value(f, &cookie, "synthetic-one", None).await?.0, 201);
        let item = listed(f, &cookie, "provider").await?;
        assert!(item["rotated_at"].is_string(), "{item}");
        assert_eq!((&item["rotate_after_days"], &item["rotation_due"]), (&Value::Null, &json!(false)));
        // Bounds and versions.
        for days in [json!(0), json!(3651), json!("30")] {
            let (status, body) = remind(f, &cookie, days.clone(), 1).await?;
            assert_eq!(
                (status, detail(&body)),
                (400, (json!("rotate_after_days"), json!("rotate_after_invalid"))),
                "{days}"
            );
        }
        assert_eq!(remind(f, &cookie, json!(30), 7).await?.0, 409);
        let (status, item) = remind(f, &cookie, json!(30), 1).await?;
        assert_eq!(status, 200, "{item}");
        assert_eq!((&item["version"], &item["rotate_after_days"], &item["rotation_due"]), (&json!(2), &json!(30), &json!(false)));
        let audited: (String, bool) = sqlx::query_as(
            "SELECT action, value_hash_changed FROM admin_audit WHERE item='secrets/provider' ORDER BY at DESC, id DESC LIMIT 1",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(audited, ("replace".to_owned(), false), "a reminder is audited, and changes no value");
        // Past its interval: due, logged and counted, and the secret still works.
        age(f, 31).await?;
        assert_eq!(listed(f, &cookie, "provider").await?["rotation_due"], json!(true));
        f.gateway.rotation_reminders_now().await?;
        assert_eq!(gauge(f).await?, "tqa_secrets_rotation_due 1");
        let text = logs.text()?;
        assert!(
            text.lines().any(|line| line.contains("WARN") && line.contains("secrets/provider") && line.contains("rotation")),
            "{text}"
        );
        assert!(!text.contains("synthetic-one"), "never a value");
        let read = f.gateway.secret_store().get(&"secrets/provider".parse()?).await?;
        assert!(read.is_some(), "a due reminder never blocks a read");
        // The same value again is not a rotation; a new value is.
        assert_eq!(value(f, &cookie, "synthetic-one", Some(2)).await?.0, 200);
        assert_eq!(listed(f, &cookie, "provider").await?["rotation_due"], json!(true));
        assert_eq!(value(f, &cookie, "synthetic-two", Some(3)).await?.0, 200);
        assert_eq!(listed(f, &cookie, "provider").await?["rotation_due"], json!(false));
        f.gateway.rotation_reminders_now().await?;
        assert_eq!(gauge(f).await?, "tqa_secrets_rotation_due 0");
        // Cleared: no reminder at all.
        let (status, item) = remind(f, &cookie, Value::Null, 4).await?;
        assert_eq!(status, 200, "{item}");
        assert_eq!((&item["rotate_after_days"], &item["rotation_due"]), (&Value::Null, &json!(false)));
        Ok(())
    })
    .await
}
