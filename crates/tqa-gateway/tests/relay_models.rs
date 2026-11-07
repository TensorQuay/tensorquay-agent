//! GW-I-112: `GET /api/v1/admin/routes/{name}/models` lists a route's upstream models with its stored credential: each
//! protocol's path, auth and headers, a Messages cursor followed, the cap, the upstream's refusals as the probe names
//! them, administrators only, a missing key refused before any request, one audit row without ids.
//! SEC-042: the credential (bearer, header or query parameter) appears in neither the answer, a failure's excerpt, the
//! audit nor the logs, even when the upstream echoes it, and a redirect is never followed.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError},
};
use support::{TestResult, call};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// One request the fake upstream received.
#[derive(Clone, Debug)]
struct Seen {
    target: String,
    headers: BTreeMap<String, String>,
}

type Answer = (u16, Vec<(String, String)>, String);
type Script = Arc<dyn Fn(&Seen) -> Answer + Send + Sync>;

/// A fake upstream that answers each request by `script` and records it.
async fn upstream(
    script: Script,
) -> Result<(String, Arc<Mutex<Vec<Seen>>>), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}/v1", listener.local_addr()?);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(serve(socket, script.clone(), log.clone()));
        }
    });
    Ok((base, seen))
}

/// Reads one request's head, records it, and writes the scripted answer.
async fn serve(mut socket: tokio::net::TcpStream, script: Script, log: Arc<Mutex<Vec<Seen>>>) {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    while !buffer.windows(4).any(|window| window == b"\r\n\r\n") {
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    }
    let request = head(&String::from_utf8_lossy(&buffer));
    log.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(request.clone());
    let (status, extra, body) = script(&request);
    let mut answer = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (name, value) in extra {
        let _ = std::fmt::Write::write_fmt(&mut answer, format_args!("{name}: {value}\r\n"));
    }
    answer.push_str("\r\n");
    let _ = socket.write_all(answer.as_bytes()).await;
    let _ = socket.write_all(body.as_bytes()).await;
}

/// The request line's target and the headers, names lowercased.
fn head(text: &str) -> Seen {
    let mut lines = text.split("\r\n");
    let target = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or("")
        .to_owned();
    let headers = lines
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    Seen { target, headers }
}

fn seen(log: &Arc<Mutex<Vec<Seen>>>) -> Vec<Seen> {
    log.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Routes of each protocol and auth kind on one upstream, and one whose key is never stored.
fn registry(base: &str) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='chat'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='m'\nauth={{kind='bearer'}}\n\
         key_ref='secrets/listing'\n\
         [[routes]]\nname='responses'\nprotocol='openai_responses'\nbase_url='{base}'\nmodel='m'\n\
         auth={{kind='header',name='api-key'}}\nkey_ref='secrets/listing'\n\
         [[routes]]\nname='messages'\nprotocol='anthropic_messages'\nbase_url='{base}'\nmodel='m'\n\
         auth={{kind='header',name='x-api-key'}}\nkey_ref='secrets/listing'\nmax_output=4096\n\
         [[routes]]\nname='query'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='m'\n\
         auth={{kind='query',name='key'}}\nkey_ref='secrets/listing'\n\
         [[routes]]\nname='spare'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='m'\nauth={{kind='bearer'}}\n\
         key_ref='secrets/listing'\n\
         [[routes]]\nname='worded'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='m'\nauth={{kind='bearer'}}\n\
         key_ref='secrets/listing'\n\
         [[routes]]\nname='unkeyed'\nprotocol='openai_chat'\nbase_url='{base}'\nmodel='m'\nauth={{kind='bearer'}}\n\
         key_ref='secrets/absent'"
    ))?)
}

/// Stores a synthetic credential and returns it, so a test can check it never appears.
async fn keyed(f: &support::Fixture) -> Result<String, Box<dyn std::error::Error>> {
    let secret = format!("synthetic-listing-key-{}", uuid::Uuid::now_v7().simple());
    f.gateway
        .secret_store()
        .put(
            &"secrets/listing".parse()?,
            secrecy::SecretString::from(secret.clone()),
        )
        .await?;
    Ok(secret)
}

async fn list(
    f: &support::Fixture,
    cookie: &str,
    route: &str,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    call(
        f,
        Method::GET,
        &format!("/api/v1/admin/routes/{route}/models"),
        cookie,
        (None, None),
    )
    .await
}

fn ids(body: &Value) -> Vec<String> {
    body["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| model["id"].as_str().map(str::to_owned))
        .collect()
}

/// Collects every tracing event's fields, so a test can read what the gateway logged.
struct Recorder(Arc<Mutex<String>>);

/// Everything this test binary's gateways logged. The recorder is the global subscriber, set once: a thread's own
/// subscriber misses events whose callsite another test registered first, while it had none.
fn logs() -> Arc<Mutex<String>> {
    static LOGS: std::sync::OnceLock<Arc<Mutex<String>>> = std::sync::OnceLock::new();
    LOGS.get_or_init(|| {
        let output = Arc::new(Mutex::new(String::new()));
        let _ = tracing::subscriber::set_global_default(Recorder(output.clone()));
        output
    })
    .clone()
}
impl tracing::Subscriber for Recorder {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Fields<'a>(&'a mut String);
        impl tracing::field::Visit for Fields<'_> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                let _ = std::fmt::Write::write_fmt(self.0, format_args!("{field}={value:?} "));
            }
        }
        let mut output = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        event.record(&mut Fields(&mut output));
        output.push('\n');
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

// TEST: GW-I-112
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_112_each_protocol_lists_with_its_own_path_auth_and_headers(
    pool: PgPool,
) -> TestResult {
    let (base, log) = upstream(Arc::new(|request: &Seen| {
        let body = if request.target.contains("after_id=m-2") {
            json!({"data": [{"id": "m-3"}], "has_more": false, "last_id": "m-3"})
        } else if request.headers.contains_key("anthropic-version") {
            json!({"data": [{"id": "m-1", "display_name": "One"}, {"id": "m-2"}], "has_more": true, "last_id": "m-2"})
        } else {
            json!({"object": "list", "data": [{"id": "model-x", "owned_by": "o"}, {"id": "model-y"}]})
        };
        (200, vec![], body.to_string())
    }))
    .await?;
    support::run_configured(pool, Some(registry(&base)?), "", async |f| {
        let secret = keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        let (status, body) = list(f, &cookie, "chat").await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            (body["route"].clone(), body["protocol"].clone()),
            (json!("chat"), json!("openai_chat"))
        );
        assert_eq!(ids(&body), ["model-x", "model-y"]);
        assert_eq!(
            (body["truncated"].clone(), body["failure"].clone()),
            (json!(false), Value::Null)
        );
        let (_, responses) = list(f, &cookie, "responses").await?;
        assert_eq!(ids(&responses), ["model-x", "model-y"]);
        let (status, messages) = list(f, &cookie, "messages").await?;
        assert_eq!(status, 200, "{messages}");
        assert_eq!(ids(&messages), ["m-1", "m-2", "m-3"]);
        assert_eq!(messages["models"][0]["display_name"], "One");
        let requests = seen(&log);
        assert_eq!(requests[0].target, "/v1/models");
        assert_eq!(
            requests[0].headers["authorization"],
            format!("Bearer {secret}")
        );
        assert_eq!(requests[1].headers["api-key"], secret);
        assert!(!requests[1].headers.contains_key("anthropic-version"));
        assert_eq!(requests[2].headers["x-api-key"], secret);
        assert!(requests[2].headers.contains_key("anthropic-version"));
        assert!(
            requests[3].target.contains("after_id=m-2"),
            "{:?}",
            requests[3]
        );
        assert_eq!(requests.len(), 4);
        // One audit row per listing, with no ids.
        let audit: Vec<(String, String)> = sqlx::query_as(
            "SELECT area, item FROM admin_audit WHERE action = 'list_models' ORDER BY id",
        )
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(audit.len(), 3);
        assert_eq!(audit[0], ("routes".into(), "chat".into()));
        let rows: String =
            sqlx::query_scalar("SELECT string_agg(row_to_json(a)::text, '') FROM admin_audit a")
                .fetch_one(&f.pool)
                .await?;
        assert!(!rows.contains("model-x") && !rows.contains(&secret));
        Ok(())
    })
    .await
}

// TEST: GW-I-112
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_112_refusals_never_reach_the_upstream_and_the_list_is_capped(
    pool: PgPool,
) -> TestResult {
    let many: Vec<Value> = (0..520)
        .map(|index| json!({"id": format!("m-{index}")}))
        .collect();
    let body = json!({"data": many}).to_string();
    let (base, log) = upstream(Arc::new(move |_| (200, vec![], body.clone()))).await?;
    support::run_configured(pool, Some(registry(&base)?), "", async |f| {
        keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        assert_eq!(list(f, &member, "chat").await?.0, 403);
        assert_eq!(list(f, &cookie, "nosuch").await?.0, 404);
        let (status, body) = list(f, &cookie, "unkeyed").await?;
        assert_eq!(
            (status, body["error"]["code"].clone()),
            (409, json!("key_missing"))
        );
        assert!(seen(&log).is_empty());
        let (status, body) = list(f, &cookie, "chat").await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(ids(&body).len(), 500);
        assert_eq!(body["truncated"], true);
        Ok(())
    })
    .await
}

// TEST: GW-I-112
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_112_the_upstreams_refusals_are_named_as_the_probe_names_them(
    pool: PgPool,
) -> TestResult {
    let status = Arc::new(Mutex::new(401_u16));
    let current = status.clone();
    let (base, _) = upstream(Arc::new(move |_| {
        let code = *current.lock().unwrap_or_else(PoisonError::into_inner);
        // A 400 worded like a strict-schema refusal: a listing sends no tools, so it never suggests a route setting.
        let body = match code {
            200 => "not a list".to_owned(),
            400 => json!({"error": {"message": "Invalid schema for function: strict mode not supported"}})
                .to_string(),
            _ => json!({"error": {"message": "no"}}).to_string(),
        };
        (code, vec![], body)
    }))
    .await?;
    support::run_configured(pool, Some(registry(&base)?), "", async |f| {
        keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        // One route per case: a route is listed at most once per 30 seconds.
        for (route, code, expected) in [
            ("chat", 401, "auth_rejected"),
            ("responses", 404, "not_found"),
            ("messages", 429, "rate_limited"),
            ("query", 503, "upstream_error"),
            ("spare", 200, "models_unreadable"),
            ("worded", 400, "upstream_error"),
        ] {
            *status.lock().unwrap_or_else(PoisonError::into_inner) = code;
            let (answer, body) = list(f, &cookie, route).await?;
            assert_eq!(answer, 200, "{body}");
            assert_eq!(body["failure"]["code"], expected, "{code}: {body}");
            assert_eq!(body["models"], json!([]), "{code}");
            assert!(
                body["failure"].get("suggestion").is_none(),
                "{code}: {body}"
            );
        }
        // A second listing of a route inside its window is refused before the upstream, with the wait.
        let (status, body) = list(f, &cookie, "chat").await?;
        assert_eq!(
            (status, body["error"]["code"].clone()),
            (429, json!("rate_limited")),
            "{body}"
        );
        Ok(())
    })
    .await
}

// TEST: SEC-042
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_042_the_credential_never_leaves_the_gateway(pool: PgPool) -> TestResult {
    // The upstream refuses and echoes everything it received (the query string and every header) into its body.
    let (base, log) = upstream(Arc::new(|request: &Seen| {
        let echo = json!({"error": {"message": format!("bad key: {} {:?}", request.target, request.headers)}});
        (401, vec![], echo.to_string())
    }))
    .await?;
    let output = logs();
    support::run_configured(pool, Some(registry(&base)?), "", async |f| {
        let secret = keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        for route in ["chat", "responses", "messages", "query"] {
            let (status, body) = list(f, &cookie, route).await?;
            assert_eq!(
                (status, body["failure"]["code"].clone()),
                (200, json!("auth_rejected")),
                "{route}: {body}"
            );
            assert!(
                !body.to_string().contains(&secret),
                "{route}: the answer carries the credential: {body}"
            );
        }
        // The query route really sent the credential in its URL, and nothing gave it back.
        assert!(
            seen(&log)
                .iter()
                .any(|request| request.target.contains(&secret))
        );
        let rows: String =
            sqlx::query_scalar("SELECT string_agg(row_to_json(a)::text, '') FROM admin_audit a")
                .fetch_one(&f.pool)
                .await?;
        assert!(!rows.contains(&secret));
        Ok(())
    })
    .await?;
    let logged = output
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert!(
        logged.contains("route models listed") && logged.contains("route=\"query\""),
        "the gateway's own line was captured: {logged}"
    );
    assert!(
        !logged.contains("synthetic-listing-key-"),
        "a credential reached the logs"
    );
    Ok(())
}

// TEST: SEC-042
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_042_a_redirect_is_not_followed(pool: PgPool) -> TestResult {
    let (base, log) = upstream(Arc::new(|_| {
        (
            302,
            vec![("location".into(), "http://127.0.0.1:9/v1/models".into())],
            String::new(),
        )
    }))
    .await?;
    support::run_configured(pool, Some(registry(&base)?), "", async |f| {
        keyed(f).await?;
        let (_, cookie) = f.admin().await?;
        let (status, body) = list(f, &cookie, "chat").await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["failure"]["code"], "upstream_error", "{body}");
        assert_eq!(seen(&log).len(), 1, "one request, no redirect followed");
        Ok(())
    })
    .await
}
