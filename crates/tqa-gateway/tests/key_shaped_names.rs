//! SEC-033 (found on 7 Oct 2026): a key pasted where a secret's name belongs. Every admin write refuses it with
//! `400 name_key_shaped` naming the field, before anything is written, bound, audited or kept for a replay, and the
//! value is in no response, table or log line: a route's `key_ref`, a connector's `secret_ref`, a decision engine's
//! `key_ref`, and a new shared key's name. A configuration file's is never refused, since an upgrade must start: it is
//! warned about by its entry, never its value; a shared key stored under such a name can still be replaced or deleted.
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

/// A made-up access key's shape, put together here so the source holds nothing key-shaped for the secrets scan (G-10).
fn pasted() -> String {
    let body: String = (0..16u8).map(|index| char::from(b'A' + index)).collect();
    format!("{}{}{body}", "AK", "IA")
}

/// The process's subscriber at the gateway's own default filter, so it also sees spawned tasks.
fn capture_logs() -> Result<log_capture::Capture, Box<dyn std::error::Error>> {
    static CAPTURE: std::sync::OnceLock<Option<log_capture::Capture>> = std::sync::OnceLock::new();
    CAPTURE
        .get_or_init(|| {
            let capture = log_capture::Capture::default();
            let filter = tqa_gateway::logging::filter(None).ok()?;
            let subscriber = tracing_subscriber::fmt()
                .with_writer(capture.clone())
                .with_ansi(false)
                .with_env_filter(filter)
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

/// How many rows anywhere a refused write could have reached hold `key`: the settings tables, the bindings, the admin
/// audit and the replay records.
async fn traces(f: &Fixture, key: &str) -> Result<i64, Box<dyn std::error::Error>> {
    let pattern = format!("%{key}%");
    let mut total = 0;
    for query in [
        "SELECT count(*) FROM model_routes WHERE document::text LIKE $1",
        "SELECT count(*) FROM connectors WHERE document::text LIKE $1",
        "SELECT count(*) FROM decision_engines WHERE document::text LIKE $1",
        "SELECT count(*) FROM secrets WHERE ref LIKE $1",
        "SELECT count(*) FROM secret_consumers WHERE ref LIKE $1",
        "SELECT count(*) FROM admin_audit WHERE item LIKE $1 OR coalesce(detail::text, '') LIKE $1",
    ] {
        total += sqlx::query_scalar::<_, i64>(query)
            .bind(&pattern)
            .fetch_one(&f.pool)
            .await?;
    }
    total += sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM idempotency WHERE position(convert_to($1, 'UTF8') in response_body) > 0",
    )
    .bind(key)
    .fetch_one(&f.pool)
    .await?;
    Ok(total)
}

// TEST: SEC-033
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_033_every_admin_write_refuses_a_key_where_a_name_belongs(pool: PgPool) -> TestResult {
    let logs = capture_logs()?;
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let key = pasted();
        let reference = format!("secrets/{key}");
        let writes = [
            (
                "/api/v1/admin/routes/pasted",
                json!({"protocol": "openai_chat", "base_url": "https://models.example/v1", "model": "m",
                       "auth": {"kind": "bearer"}, "key_ref": reference}),
                "key_ref",
            ),
            (
                "/api/v1/admin/connectors/pasted",
                json!({"url": "https://mcp.example/mcp", "auth": {"scheme": "bearer"}, "secret_ref": reference}),
                "secret_ref",
            ),
            (
                "/api/v1/admin/decision/engines/pasted",
                json!({"url": "https://engine.example", "auth": {"scheme": "bearer"}, "key_ref": reference,
                       "timeout_ms": 500, "exposure": "calls"}),
                "key_ref",
            ),
            (
                &format!("/api/v1/admin/secrets/{key}"),
                json!({"value": "synthetic-value"}),
                "name",
            ),
        ];
        for (index, (path, body, field)) in writes.iter().enumerate() {
            let replay = format!("pasted-{index}");
            let (status, answer) =
                call(f, Method::PUT, path, &cookie, (Some(body.clone()), Some(&replay))).await?;
            assert_eq!(status, 400, "{path}: {answer}");
            assert_eq!(detail(&answer), (json!(field), json!("name_key_shaped")), "{path}");
            assert!(!answer.to_string().contains(&key), "{path} echoed the value");
        }
        assert_eq!(traces(f, &key).await?, 0, "a refused write left the key somewhere");
        // The export is TOML, read as text.
        let response = f
            .client
            .get(f.url("/api/v1/admin/routes/export"))
            .header("cookie", &cookie)
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        assert!(!response.text().await?.contains(&key), "the export names the key");
        Ok(())
    })
    .await?;
    assert!(
        !logs.text()?.contains(&pasted()),
        "the key reached the gateway log"
    );
    Ok(())
}

// TEST: SEC-033
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_033_a_readable_name_is_still_taken(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        // The route first: naming a secret that doesn't exist yet binds it to the route; naming an existing,
        // unbound one would be `secret_not_allowed`.
        let body = json!({"protocol": "openai_chat", "base_url": "https://models.example/v1", "model": "m",
                          "auth": {"kind": "bearer"}, "key_ref": "secrets/hf_token"});
        let (status, answer) =
            call(f, Method::PUT, "/api/v1/admin/routes/readable", &cookie, (Some(body), None)).await?;
        assert_eq!(status, 201, "{answer}");
        // Close to the shapes, still names (the corpus in tqa-types' key_shapes_tests.rs has the rest).
        for name in ["hf_token", "OpenAIKeyForTeam2026Prod", "provider-gateway-shared-production-key"] {
            let path = format!("/api/v1/admin/secrets/{name}");
            let (status, answer) =
                call(f, Method::PUT, &path, &cookie, (Some(json!({"value": "synthetic"})), None)).await?;
            assert_eq!(status, 201, "{name}: {answer}");
        }
        Ok(())
    })
    .await
}

// TEST: SEC-033
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_033_files_are_warned_about_by_entry_and_a_stored_key_name_can_be_replaced(
    pool: PgPool,
) -> TestResult {
    let logs = capture_logs()?;
    let key = pasted();
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='from-file'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='m'\n\
         auth={{kind='bearer'}}\nkey_ref='secrets/{key}'\n"
    ))?;
    support::run_restarting_with(pool, Some(registry), "", async |f| {
        // The secrets seed file stores a key under a key-shaped name: the start goes on.
        f.seed_secrets(&format!(
            "[secrets]\n\"secrets/{key}\"='synthetic-seed-value'\n"
        ))?;
        f.restart().await?;
        let (_, cookie) = f.admin().await?;
        let (status, routes) = call(
            f,
            Method::GET,
            "/api/v1/admin/routes",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        assert!(
            routes.to_string().contains("from-file"),
            "the file's route is kept"
        );
        // Stored before the check, it can be given a new value and deleted (only a create is refused).
        let path = format!("/api/v1/admin/secrets/{key}");
        let (status, listed) = call(
            f,
            Method::GET,
            "/api/v1/admin/secrets",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        let version = listed["items"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["ref"] == json!(format!("secrets/{key}")))
            })
            .map(|item| item["version"].clone())
            .ok_or("the seeded secret is listed")?;
        let (status, answer) = call(
            f,
            Method::PUT,
            &path,
            &cookie,
            (
                Some(json!({"value": "synthetic-replacement", "version": version})),
                None,
            ),
        )
        .await?;
        assert_eq!(status, 200, "{answer}");
        Ok(())
    })
    .await?;
    let text = logs.text()?;
    assert!(
        text.contains("looks like a key, not a name"),
        "the file's entries are warned about"
    );
    assert!(text.contains("from-file"), "the route is named");
    assert!(!text.contains(&key), "the value never reaches the log");
    Ok(())
}
