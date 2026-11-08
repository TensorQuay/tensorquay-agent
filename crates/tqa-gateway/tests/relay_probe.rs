//! GW-I-037 (API and storage): `POST /api/v1/admin/routes/{name}/probe` tests a configured route through the
//! relay against the scripted fake model, stores the latest observation per route with the digest of the document it
//! tested, audits it without values, and never shows the credential. The engine's own cases are in
//! `relay/probe_tests.rs`; admission (running and rate limits) is unit-tested in `relay/probe_api_tests.rs`.
//! GW-I-099: a refusal the probe recognises is stored with a suggested route setting, and the route is left as it is.
//! GW-I-140: the route list shows each route's stored check, and a check of an older document as stale.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use sqlx::PgPool;
use std::fmt::Write as _;
use support::{TestResult, call};
use tqa_testkit::{FakeModelServer, Script, Scripted};

/// An audit row: area, item, versions before and after, and whether the value changed.
type AuditRow = (String, String, Option<i64>, Option<i64>, bool);

/// The probe's nonce for an Idempotency-Key: the first eight hex digits of SHA-256 over the fixed prefix and the key.
fn nonce(key: &str) -> String {
    let digest = Sha256::digest(format!("tqa-route-probe:{key}").as_bytes());
    digest.iter().take(4).fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

fn usage() -> Scripted {
    Scripted::Usage(tqa_llm::Usage {
        prompt_tokens: 20,
        completion_tokens: 5,
        cached_tokens: None,
        reasoning_tokens: None,
    })
}

/// A model that calls the probe tool with the nonce for `key`, then answers, then keeps the strict-string check's line
/// break (GW-U-064).
fn good(key: &str) -> Vec<Script> {
    vec![
        Script::new(vec![
            Scripted::ToolCall {
                name: "probe_echo".into(),
                arguments_json: json!({ "value": nonce(key) }).to_string(),
                split_into: 1,
            },
            usage(),
        ]),
        Script::new(vec![Scripted::Text("ok".into()), usage()]),
        Script::new(vec![
            Scripted::ToolCall {
                name: "probe_lines".into(),
                arguments_json: json!({ "value": format!("{0}\n{0}", nonce(key)) }).to_string(),
                split_into: 1,
            },
            usage(),
        ]),
    ]
}

/// `probed` (native tools, keyed on the seeded `secrets/local`) and `unkeyed` (a key reference with no value).
fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='probed'\nprotocol='openai_chat'\nbase_url='{url}'\nmodel='m'\nauth={{kind='bearer'}}\n\
         key_ref='secrets/local'\nstream_usage=true\nmax_output=4096\n\
         [[routes]]\nname='unkeyed'\nprotocol='openai_chat'\nbase_url='{url}'\nmodel='m'\nauth={{kind='bearer'}}\n\
         key_ref='secrets/absent'\nstream_usage=true\nmax_output=4096",
        url = server.base_url()
    ))?)
}

/// Stores a synthetic credential for `probed` and returns it, so a test can check it never appears.
async fn keyed(f: &support::Fixture) -> Result<String, Box<dyn std::error::Error>> {
    let secret = format!("synthetic-probe-key-{}", uuid::Uuid::now_v7().simple());
    f.gateway
        .secret_store()
        .put(
            &"secrets/local".parse()?,
            secrecy::SecretString::from(secret.clone()),
        )
        .await?;
    Ok(secret)
}

async fn probe(
    f: &support::Fixture,
    cookie: &str,
    route: &str,
    key: Option<&str>,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    call(
        f,
        Method::POST,
        &format!("/api/v1/admin/routes/{route}/probe"),
        cookie,
        (None, key),
    )
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_037_a_good_route_is_verified_stored_and_audited_without_values(
    pool: PgPool,
) -> TestResult {
    let key = "probe-good-1";
    let server =
        FakeModelServer::start(good(key), std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        let secret = keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        let (status, body) = probe(f, &cookie, "probed", Some(key)).await?;
        assert_eq!(status, 200, "{body}");
        let result = &body["probe"];
        assert_eq!(result["state"], "verified", "{body}");
        assert_eq!(result["observed"]["ok"], true);
        assert_eq!(
            result["observed"]["tokens"],
            json!({"prompt": 60, "completion": 15})
        );
        assert_eq!(result["observed"]["strict_line_breaks"], "passed");
        assert_eq!(result["route_version"], 1);
        assert!(result["probed_at"].is_string());
        assert_eq!(server.requests().len(), 3);
        // An idempotent replay returns the stored result and does not probe again.
        let (again, replay) = probe(f, &cookie, "probed", Some(key)).await?;
        assert_eq!((again, &replay), (200, &body));
        assert_eq!(server.requests().len(), 3);
        // The stored observation names the document it tested.
        let (stored_ok, digest_matches): (bool, bool) = sqlx::query_as(
            "SELECT p.ok, p.document_digest = sha256(convert_to(r.document::text, 'UTF8')) \
             FROM route_probes p JOIN model_routes r ON r.name = p.route WHERE p.route = 'probed'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!((stored_ok, digest_matches), (true, true));
        // One audit row: area routes, action probe, the tested version on both sides, no value change.
        let audit: Vec<AuditRow> = sqlx::query_as(
            "SELECT area, item, before_version, after_version, value_hash_changed FROM admin_audit \
             WHERE action = 'probe'",
        )
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(
            audit,
            vec![("routes".into(), "probed".into(), Some(1), Some(1), false)]
        );
        // The credential appears nowhere in the result or the stored observation.
        let observed: String = sqlx::query_scalar("SELECT observed::text FROM route_probes")
            .fetch_one(&f.pool)
            .await?;
        assert!(!body.to_string().contains(&secret) && !observed.contains(&secret));
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_037_refusals_never_contact_the_model(pool: PgPool) -> TestResult {
    let server =
        FakeModelServer::start(good("unused"), std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        let error = |body: &Value| body["error"]["code"].clone();
        let (status, body) = probe(f, &cookie, "probed", None).await?;
        assert_eq!(status, 400, "the Idempotency-Key is required: {body}");
        let (status, _) = probe(f, &member, "probed", Some("member-1")).await?;
        assert_eq!(status, 403);
        let (status, _) = probe(f, &cookie, "nosuch", Some("unknown-1")).await?;
        assert_eq!(status, 404);
        let (status, body) = probe(f, &cookie, "unkeyed", Some("unkeyed-1")).await?;
        assert_eq!((status, error(&body)), (409, json!("key_missing")));
        assert!(server.requests().is_empty());
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_037_a_failed_probe_is_an_observation_and_a_route_is_rate_limited(
    pool: PgPool,
) -> TestResult {
    // The model answers text where the route declares native tools: the probe runs and fails with a named code.
    let text = vec![Script::new(vec![
        Scripted::Text("no tools here".into()),
        usage(),
    ])];
    let server = FakeModelServer::start(text, std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        let (status, body) = probe(f, &cookie, "probed", Some("text-1")).await?;
        assert_eq!(status, 200, "a failed probe is still a result: {body}");
        assert_eq!(body["probe"]["state"], "failed");
        assert_eq!(body["probe"]["observed"]["ok"], false);
        assert_eq!(body["probe"]["observed"]["failure"]["code"], "no_tool_call");
        // A second probe of the same route inside the window is refused with a wait, and never reaches the model.
        let requests = server.requests().len();
        let response = f
            .client
            .post(f.url("/api/v1/admin/routes/probed/probe"))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .header("idempotency-key", "text-2")
            .send()
            .await?;
        assert_eq!(response.status(), 429);
        let wait: u64 = response
            .headers()
            .get("retry-after")
            .ok_or("missing Retry-After")?
            .to_str()?
            .parse()?;
        assert!((1..=30).contains(&wait), "Retry-After {wait}");
        let body: Value = response.json().await?;
        assert_eq!(body["error"]["code"], "rate_limited");
        assert_eq!(server.requests().len(), requests);
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_037_a_changed_route_is_probed_at_once_and_its_repeat_waits(
    pool: PgPool,
) -> TestResult {
    // The lead, 12 Oct 2026: a route saved again (a new version, as after a check's suggestion) is a new thing to
    // check; a second check of the same version still waits its 30 s.
    let text = || Script::new(vec![Scripted::Text("no tools here".into()), usage()]);
    let server = FakeModelServer::start(
        vec![text(), text()],
        std::sync::Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let url = server.base_url();
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        let (status, body) = probe(f, &cookie, "probed", Some("change-1")).await?;
        assert_eq!(status, 200, "{body}");
        let (status, body) = probe(f, &cookie, "probed", Some("change-2")).await?;
        assert_eq!((status, &body["error"]["code"]), (429, &json!("rate_limited")), "{body}");
        // Saved again with a change: its first check starts at once and reaches the model.
        let route = listed(f, &cookie, "probed").await?;
        let replace = json!({"protocol": "openai_chat", "base_url": url, "model": "m2", "auth": {"kind": "bearer"},
            "key_ref": "secrets/local", "stream_usage": true, "max_output": 4096, "version": route["version"]});
        let (status, changed) = call(
            f,
            Method::PUT,
            "/api/v1/admin/routes/probed",
            &cookie,
            (Some(replace), None),
        )
        .await?;
        assert_eq!(status, 200, "{changed}");
        let requests = server.requests().len();
        let (status, body) = probe(f, &cookie, "probed", Some("change-3")).await?;
        assert_eq!(status, 200, "a changed route is checked at once: {body}");
        assert!(server.requests().len() > requests, "the check reached the model");
        // Its repeat waits again.
        let (status, body) = probe(f, &cookie, "probed", Some("change-4")).await?;
        assert_eq!((status, &body["error"]["code"]), (429, &json!("rate_limited")), "{body}");
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_037_a_route_already_being_probed_refuses_a_second_probe(pool: PgPool) -> TestResult {
    // The model holds the first probe's answer until the fake clock moves, so it is deterministically in flight.
    let clock = tqa_testkit::FakeClock::new();
    let mut scripts = good("inflight-1");
    scripts[0].header_delay = std::time::Duration::from_secs(5);
    let sleeper = std::sync::Arc::new(tqa_testkit::FakeSleeper::new(clock.clone()));
    let server = FakeModelServer::start(scripts, sleeper).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        let first = f
            .client
            .post(f.url("/api/v1/admin/routes/probed/probe"))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .header("idempotency-key", "inflight-1")
            .send();
        let first = tokio::spawn(first);
        // Bounded wait for the model to receive the first request (a real local HTTP hop, no sleep).
        tokio::time::timeout(support::DEADLINE, async {
            while server.requests().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let (status, body) = probe(f, &cookie, "probed", Some("inflight-2")).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (409, &json!("probe_running"))
        );
        assert_eq!(
            server.requests().len(),
            1,
            "the refused probe never reached the model"
        );
        clock.advance(std::time::Duration::from_secs(5))?;
        let response = first.await??;
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await?;
        assert_eq!(body["probe"]["state"], "verified", "{body}");
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_099_a_recognised_refusal_is_stored_with_a_suggestion_and_the_route_is_unchanged(
    pool: PgPool,
) -> TestResult {
    // The probe tool's schema is open; a route set to send every tool strict has it refused by a strict endpoint.
    let server = FakeModelServer::start_as(
        tqa_testkit::FakeProfile::DeepseekChat,
        vec![],
        std::sync::Arc::new(tqa_llm::TokioSleeper),
    )
    .await?;
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='probed'\nprotocol='openai_chat'\nbase_url='{url}'\nmodel='m'\nauth={{kind='bearer'}}\n\
         key_ref='secrets/local'\nstream_usage=true\nmax_output=4096\nstrict='on'",
        url = server.base_url()
    ))?;
    let result = support::run_configured(pool, Some(registry), "", async |f| {
        keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        let (status, body) = probe(f, &cookie, "probed", Some("strict-1")).await?;
        assert_eq!(status, 200, "{body}");
        let failure = &body["probe"]["observed"]["failure"];
        assert_eq!(body["probe"]["state"], "failed", "{body}");
        assert_eq!(failure["code"], "upstream_error");
        assert_eq!(failure["status"], 400);
        assert_eq!(failure["suggestion"]["field"], "strict");
        assert_eq!(failure["suggestion"]["value"], "off");
        assert!(failure["suggestion"]["reason"].is_string());
        assert_eq!(server.requests().len(), 1);
        let stored: Value =
            sqlx::query_scalar("SELECT observed FROM route_probes WHERE route = 'probed'")
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(stored["failure"]["suggestion"], failure["suggestion"]);
        // A hint only: the route keeps its version and its setting.
        let (version, strict): (i64, Option<String>) = sqlx::query_as(
            "SELECT version, document->>'strict' FROM model_routes WHERE name = 'probed'",
        )
        .fetch_one(&f.pool)
        .await?;
        assert_eq!((version, strict.as_deref()), (1, Some("on")));
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

/// The route `name` as `GET /api/v1/admin/routes` lists it.
async fn listed(
    f: &support::Fixture,
    cookie: &str,
    name: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let (status, body) = call(f, Method::GET, "/api/v1/admin/routes", cookie, (None, None)).await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["items"]
        .as_array()
        .ok_or("no items")?
        .iter()
        .find(|item| item["name"] == name)
        .cloned()
        .ok_or(format!("{name} is not listed"))?)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_140_the_route_list_shows_the_stored_check_and_a_changed_route_as_stale(
    pool: PgPool,
) -> TestResult {
    let key = "listed-good-1";
    let server =
        FakeModelServer::start(good(key), std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    let url = server.base_url();
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        // Before any check, every route lists as never checked.
        assert_eq!(
            listed(f, &cookie, "probed").await?["probe"],
            json!({"state": "never", "probed_at": null, "observed": null})
        );
        let (status, body) = probe(f, &cookie, "probed", Some(key)).await?;
        assert_eq!(status, 200, "{body}");
        // The list shows what the check stored: its state, when, and what it observed.
        let route = listed(f, &cookie, "probed").await?;
        assert_eq!(
            route["probe"],
            json!({"state": "verified", "probed_at": body["probe"]["probed_at"],
                "observed": body["probe"]["observed"]}),
        );
        assert_eq!(listed(f, &cookie, "unkeyed").await?["probe"]["state"], "never");
        // A change to the route's document leaves the check describing the old one: stale, with its time kept.
        let replace = json!({"protocol": "openai_chat", "base_url": url, "model": "m2", "auth": {"kind": "bearer"},
            "key_ref": "secrets/local", "stream_usage": true, "max_output": 4096, "version": route["version"]});
        let (status, changed) = call(
            f,
            Method::PUT,
            "/api/v1/admin/routes/probed",
            &cookie,
            (Some(replace), None),
        )
        .await?;
        assert_eq!(status, 200, "{changed}");
        assert_eq!(changed["probe"]["state"], "stale", "the write's reply shows it too");
        let probe = &listed(f, &cookie, "probed").await?["probe"];
        assert_eq!(
            (&probe["state"], &probe["probed_at"]),
            (&json!("stale"), &body["probe"]["probed_at"])
        );
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_140_the_route_list_shows_a_failed_check(pool: PgPool) -> TestResult {
    let text = vec![Script::new(vec![
        Scripted::Text("no tools here".into()),
        usage(),
    ])];
    let server = FakeModelServer::start(text, std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    let result = support::run_configured(pool, Some(registry(&server)?), "", async |f| {
        keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        let (status, body) = probe(f, &cookie, "probed", Some("listed-text-1")).await?;
        assert_eq!(
            (status, &body["probe"]["state"]),
            (200, &json!("failed")),
            "{body}"
        );
        let probe = &listed(f, &cookie, "probed").await?["probe"];
        assert_eq!(
            (
                &probe["state"],
                &probe["probed_at"],
                &probe["observed"]["failure"]["code"]
            ),
            (
                &json!("failed"),
                &body["probe"]["probed_at"],
                &json!("no_tool_call")
            )
        );
        Ok(())
    })
    .await;
    server.shutdown().await?;
    result
}

// TEST: GW-I-153
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_153_a_check_slower_than_the_io_timeout_still_answers_with_its_result(
    pool: PgPool,
) -> TestResult {
    // A check lasts up to its own deadline (a minute, or up to 10 minutes for a local model that keeps answering), far
    // past the general `[limits] io_timeout` (10 s by default): seen live, a thinking model's check was cut at 10 s
    // with a 504. Here the io_timeout is 1 s and the model's first answer takes 2.
    let key = "probe-slow-1";
    let mut scripts = good(key);
    scripts[0].header_delay = std::time::Duration::from_secs(2);
    let server =
        FakeModelServer::start(scripts, std::sync::Arc::new(tqa_llm::TokioSleeper)).await?;
    let result = support::run_configured(
        pool,
        Some(registry(&server)?),
        "[limits]\nio_timeout=1\n",
        async |f| {
            keyed(f).await?;
            let (_, cookie) = f.admin().await?;
            let (status, body) = probe(f, &cookie, "probed", Some(key)).await?;
            assert_eq!(status, 200, "{body}");
            assert_eq!(body["probe"]["state"], "verified", "{body}");
            Ok(())
        },
    )
    .await;
    server.shutdown().await?;
    result
}
