//! A route's `key_ref` names a secret; it is never the secret itself (SEC-033, GW-I-097).
//! - SEC-033: a raw key, or anything else that is not `secrets/<name>`, is refused by the API, the models file and so
//!   start-up, and the value never appears in an answer or a log.
//! - GW-I-097: a route stored before the fix with a raw key is left out of the registry and flagged, never used or
//!   echoed, and saving it with a reference makes it usable again.
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

/// Synthetic, but shaped like a provider key.
const RAW: &str = "sk-live-SYNTHETIC0123456789abcdef";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// One global capture for the process: the flag is logged from the gateway's start-up.
fn capture_logs() -> Result<log_capture::Capture> {
    static CAPTURE: std::sync::OnceLock<Option<log_capture::Capture>> = std::sync::OnceLock::new();
    CAPTURE
        .get_or_init(|| {
            let capture = log_capture::Capture::default();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(capture.clone())
                .with_ansi(false)
                .with_max_level(tracing::Level::INFO)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .ok()
                .map(|()| capture)
        })
        .clone()
        .ok_or_else(|| "another global tracing subscriber was already set".into())
}

fn route(key_ref: &str, fallbacks: &[&str]) -> Value {
    json!({"protocol": "openai_chat", "base_url": "https://models.example/v1", "model": "m",
           "auth": {"kind": "bearer"}, "key_ref": key_ref, "fallbacks": fallbacks})
}

fn detail(body: &Value) -> (Value, Value) {
    let detail = &body["error"]["details"][0];
    (detail["field"].clone(), detail["reason"].clone())
}

async fn put(f: &Fixture, cookie: &str, name: &str, body: Value) -> Result<(u16, Value)> {
    call(
        f,
        Method::PUT,
        &format!("/api/v1/admin/routes/{name}"),
        cookie,
        (Some(body), None),
    )
    .await
}

async fn listed(f: &Fixture, cookie: &str) -> Result<Vec<Value>> {
    let (status, body) = call(f, Method::GET, "/api/v1/admin/routes", cookie, (None, None)).await?;
    assert_eq!(status, 200, "{body}");
    assert!(!body.to_string().contains(RAW), "never echoed: {body}");
    Ok(body["items"].as_array().ok_or("items")?.clone())
}

async fn exported(f: &Fixture, cookie: &str) -> Result<String> {
    let response = f
        .client
        .get(f.url("/api/v1/admin/routes/export"))
        .header("cookie", cookie)
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    Ok(response.text().await?)
}

// TEST: SEC-033
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_033_the_api_refuses_a_key_ref_that_is_not_a_reference_without_echoing_it(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let long = format!("secrets/{}", "n".repeat(121));
        for key_ref in [
            RAW,
            "https://keys.example/k",
            "secrets/has space",
            "secrets/",
            long.as_str(),
            "Bearer abc",
        ] {
            let (status, body) = put(f, &cookie, "pasted", route(key_ref, &[])).await?;
            assert_eq!(
                (status, detail(&body)),
                (400, (json!("key_ref"), json!("key_ref_not_reference"))),
                "{key_ref}"
            );
            assert!(
                !body.to_string().contains(key_ref),
                "the value is never echoed: {body}"
            );
        }
        // A reference is still accepted.
        assert_eq!(
            put(f, &cookie, "pasted", route("secrets/provider-key", &[]))
                .await?
                .0,
            201
        );
        Ok(())
    })
    .await
}

// TEST: SEC-033
#[test]
fn sec_033_a_models_file_with_a_raw_key_fails_for_the_same_reason_without_echoing_it() -> TestResult
{
    let file = format!(
        "[[routes]]\nname='pasted'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='m'\n\
         auth={{kind='bearer'}}\nkey_ref='{RAW}'\n"
    );
    let error = tqa_llm::Registry::from_toml(&file)
        .err()
        .ok_or("the models file accepts a raw key")?;
    assert_eq!(
        error.reason().map(tqa_llm::RouteInvalid::code),
        Some("key_ref_not_reference")
    );
    assert!(!error.to_string().contains(RAW), "{error}");
    assert!(!format!("{error:?}").contains(RAW), "{error:?}");
    Ok(())
}

// TEST: GW-I-097
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_097_a_stored_route_with_a_raw_key_is_flagged_and_left_out_until_fixed(
    pool: PgPool,
) -> TestResult {
    let logs = capture_logs()?;
    support::run_restarting(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        // Both created with an Idempotency-Key: their responses are cached, as the old gateway cached them.
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/admin/routes/legacy",
            &cookie,
            (Some(route("secrets/legacy-key", &[])), Some("legacy-create")),
        )
        .await?;
        assert_eq!(status, 201);
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/admin/routes/main",
            &cookie,
            (Some(route("secrets/main-key", &["legacy"])), Some("main-create")),
        )
        .await?;
        assert_eq!(status, 201);
        // As a gateway before the fix stored a pasted key: in the route and in its cached response.
        sqlx::query("UPDATE model_routes SET document = jsonb_set(document, '{key_ref}', to_jsonb($1::text)) WHERE name='legacy'")
            .bind(RAW)
            .execute(&f.pool)
            .await?;
        sqlx::query(
            "UPDATE idempotency SET response_body = convert_to(replace(convert_from(response_body,'UTF8'),'secrets/legacy-key',$1),'UTF8') \
             WHERE position(convert_to('secrets/legacy-key','UTF8') in response_body) > 0",
        )
        .bind(RAW)
        .execute(&f.pool)
        .await?;
        f.restart().await?;
        // Scrubbed in the store: no copy of the value is left, and other cached responses stay.
        let holding: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM idempotency WHERE position(convert_to($1,'UTF8') in response_body) > 0) \
                  + (SELECT count(*) FROM model_routes WHERE document::text LIKE '%' || $1 || '%')",
        )
        .bind(RAW)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(holding, 0, "no copy of the pasted key is left in the database");
        let kept: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM idempotency WHERE position(convert_to('secrets/main-key','UTF8') in response_body) > 0",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(kept, 1, "the other route's cached response is kept");
        let items = listed(f, &cookie).await?;
        let legacy = items.iter().find(|item| item["name"] == "legacy").ok_or("legacy listed")?;
        assert_eq!((&legacy["key_ref"], &legacy["key_ref_invalid"]), (&Value::Null, &json!(true)), "{legacy}");
        let main = items.iter().find(|item| item["name"] == "main").ok_or("main listed")?;
        assert_eq!(main["key_ref_invalid"], json!(false), "{main}");
        let export = exported(f, &cookie).await?;
        assert!(!export.contains(RAW) && !export.contains("legacy"), "{export}");
        assert!(export.contains("name = \"main\""), "the other route keeps working: {export}");
        let text = logs.text()?;
        assert!(
            text.lines().any(|line| line.contains("ERROR") && line.contains("legacy") && line.contains("key_ref")),
            "{text}"
        );
        assert!(!text.contains(RAW), "never logged");
        // Saved with a reference, it is usable again.
        let version = legacy["version"].as_i64().ok_or("version")?;
        let mut fixed = route("secrets/legacy-key", &[]);
        fixed["version"] = json!(version);
        let (status, body) = put(f, &cookie, "legacy", fixed).await?;
        assert_eq!(status, 200, "{body}");
        assert!(exported(f, &cookie).await?.contains("name = \"legacy\""));
        Ok(())
    })
    .await
}
