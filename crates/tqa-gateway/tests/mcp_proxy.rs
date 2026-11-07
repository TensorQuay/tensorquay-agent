//! GW-I-069 and SEC-024 (ADR-0031): the MCP proxy against an in-process fake remote MCP server. The proxy checks the
//! caller, the conversation's connector set, the methods and the tool filter itself; injects the user's own
//! credential or the shared one and never the runtime token; filters tool lists; and refuses private addresses,
//! redirects, streams, oversized and slow answers without passing them through.
#[path = "support/fixture.rs"]
mod support;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// How the fake remote answers the next requests.
#[derive(Clone, Copy)]
enum Mode {
    Json,
    ServerError,
    Redirect,
    Stream,
    Huge,
    Slow,
    Garbage,
    Empty,
    HugeStream,
    Hold,
    /// A 401 with a plain-text body, as many servers answer a bad key.
    Unauthorized,
    /// A 403 with a JSON-RPC error body.
    Forbidden,
}

#[derive(Default)]
struct Seen {
    requests: Vec<(HeaderMap, Value)>,
}

struct Remote {
    /// Held calls wait here until the test releases them.
    release: tokio::sync::Notify,
    mode: Mutex<Option<Mode>>,
    seen: Mutex<Seen>,
    redirected: Mutex<u32>,
}

async fn answer(State(remote): State<Arc<Remote>>, headers: HeaderMap, body: Bytes) -> Response {
    let message: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    if let Ok(mut seen) = remote.seen.lock() {
        seen.requests.push((headers, message.clone()));
    }
    let mode = remote
        .mode
        .lock()
        .ok()
        .and_then(|mode| *mode)
        .unwrap_or(Mode::Json);
    let mut response = match mode {
        Mode::ServerError => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        Mode::Unauthorized => (StatusCode::UNAUTHORIZED, "invalid api key").into_response(),
        Mode::Forbidden => (
            StatusCode::FORBIDDEN,
            axum::Json(json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32001, "message": "key revoked"}})),
        )
            .into_response(),
        Mode::Redirect => (
            StatusCode::TEMPORARY_REDIRECT,
            [("location", "http://169.254.169.254/latest")],
        )
            .into_response(),
        Mode::Stream => {
            let id = message.get("id").cloned().unwrap_or(Value::Null);
            let tools = json!({"jsonrpc": "2.0", "id": id, "result": {"tools": [{"name": "search"}, {"name": "delete_issue"}]}});
            (
                [
                    ("content-type", "text/event-stream"),
                    ("mcp-session-id", "remote-session"),
                ],
                format!(": keep-alive\n\nevent: message\ndata: {tools}\n\n"),
            )
                .into_response()
        }
        Mode::HugeStream => (
            [("content-type", "text/event-stream")],
            format!("data: {{\"pad\": \"{}\"}}\n\n", "x".repeat(1 << 20)),
        )
            .into_response(),
        Mode::Hold => {
            remote.release.notified().await;
            axum::Json(json!({"jsonrpc": "2.0", "id": 1, "result": {}})).into_response()
        }
        Mode::Huge => {
            axum::Json(json!({"jsonrpc": "2.0", "id": 1, "result": {"pad": "x".repeat(1 << 20)}}))
                .into_response()
        }
        Mode::Garbage => ([("content-type", "application/json")], "not json").into_response(),
        Mode::Empty => StatusCode::ACCEPTED.into_response(),
        Mode::Slow => {
            tokio::time::sleep(Duration::from_secs(3)).await;
            StatusCode::OK.into_response()
        }
        Mode::Json => {
            let id = message.get("id").cloned().unwrap_or(Value::Null);
            let result = match message.get("method").and_then(Value::as_str) {
                Some("tools/list") => {
                    json!({"tools": [{"name": "search"}, {"name": "delete_issue", "annotations": {"destructiveHint": true}}]})
                }
                Some("tools/call") => json!({"content": [{"type": "text", "text": "done"}]}),
                _ => json!({}),
            };
            (
                [
                    ("mcp-session-id", "remote-session"),
                    ("set-cookie", "tracker=1"),
                ],
                axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result})),
            )
                .into_response()
        }
    };
    // GW-I-148: a remote can't pass off its answer as the gateway's own refusal.
    response.headers_mut().insert(
        "x-tqa-gateway-error",
        axum::http::HeaderValue::from_static("key_use_unrecorded"),
    );
    response
}

async fn counted(State(remote): State<Arc<Remote>>) -> StatusCode {
    if let Ok(mut count) = remote.redirected.lock() {
        *count += 1;
    }
    StatusCode::OK
}

async fn remote() -> Result<(Arc<Remote>, SocketAddr)> {
    let remote = Arc::new(Remote {
        release: tokio::sync::Notify::new(),
        mode: Mutex::new(None),
        seen: Mutex::new(Seen::default()),
        redirected: Mutex::new(0),
    });
    let app = Router::new()
        .route("/mcp", post(answer))
        .route("/latest", post(counted).get(counted))
        .with_state(remote.clone());
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok((remote, address))
}

fn seed(address: SocketAddr) -> String {
    format!(
        "[[connectors]]\nname='tracker'\nurl='http://{address}/mcp'\ninsecure_http=true\nauth={{scheme='bearer'}}\n\
         secret_ref='secrets/tracker-shared'\ntools={{deny=['delete_issue']}}\ncall_timeout_seconds=1\n\
         [[connectors]]\nname='locked'\nurl='http://{address}/mcp'\ninsecure_http=true\nauth={{scheme='header',header='X-Api-Key',required=true}}\n\
         [[connectors]]\nname='other'\nurl='http://{address}/mcp'\ninsecure_http=true\n\
         [[connectors]]\nname='off'\nurl='http://{address}/mcp'\ninsecure_http=true\nenabled=false\n\
         [[connectors]]\nname='private_ca'\nurl='http://{address}/mcp'\ninsecure_http=true\nca_file='/nonexistent/tqa-ca.pem'\n"
    )
}

/// A conversation of the administrator with `connectors` in its set, and a runtime token bound to this address.
async fn runtime(f: &Fixture, connectors: &[&str]) -> Result<(uuid::Uuid, String, String)> {
    let (owner, cookie) = f.admin().await?;
    let token = runtime_for(f, owner.id, connectors).await?;
    Ok((owner.id, cookie, token))
}

/// A conversation of `owner` with `connectors` in its set, and its runtime token.
async fn runtime_for(f: &Fixture, owner: uuid::Uuid, connectors: &[&str]) -> Result<String> {
    let conversation = f.conversation(owner).await?;
    let id = uuid::Uuid::parse_str(conversation.id.as_str())?;
    let set: Vec<String> = connectors.iter().map(|name| (*name).to_owned()).collect();
    sqlx::query("UPDATE conversations SET connectors=$2 WHERE id=$1")
        .bind(id)
        .bind(&set)
        .execute(&f.pool)
        .await?;
    let token = f
        .gateway
        .runtime_tokens()
        .mint(
            &conversation.id,
            &format!("mcp-{}", &id.simple().to_string()[20..]).parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    Ok(token.expose_secret().to_owned())
}

async fn mcp(
    f: &Fixture,
    token: Option<&str>,
    connector: &str,
    body: &Value,
) -> Result<(u16, HeaderMap, Value)> {
    let mut request = f
        .client
        .post(f.url(&format!("/mcp/{connector}")))
        .json(body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request.header("cookie", "stolen=1").send().await?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let text = response.text().await?;
    let body = serde_json::from_str(&text).unwrap_or(Value::Null);
    support::gateway_refusal(&headers, &body)?;
    Ok((status, headers, body))
}

async fn metrics(f: &Fixture) -> Result<String> {
    use tower::ServiceExt;
    let response = f
        .gateway
        .metrics_router()
        .oneshot(axum::http::Request::get("/metrics").body(axum::body::Body::empty())?)
        .await?;
    Ok(String::from_utf8(
        http_body_util::BodyExt::collect(response.into_body())
            .await?
            .to_bytes()
            .to_vec(),
    )?)
}

fn list() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
}

fn tool(name: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": name, "arguments": {}}})
}

fn requests(remote: &Remote) -> usize {
    remote.seen.lock().map_or(0, |seen| seen.requests.len())
}

fn last_authorization(remote: &Remote) -> Option<String> {
    remote.seen.lock().ok().and_then(|seen| {
        seen.requests.last().and_then(|(headers, _)| {
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        })
    })
}

// TEST: GW-I-069
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_069_calls_reach_the_remote_with_the_right_credential_and_filtered_tools(
    pool: PgPool,
) -> TestResult {
    let (remote, address) = remote().await?;
    support::run_configured(pool, None, &seed(address), async |f| {
        let (_, cookie, token) = runtime(f, &["tracker", "locked"]).await?;
        let (status, _) = call(f, Method::PUT, "/api/v1/admin/secrets/tracker-shared", &cookie, (Some(json!({"value": "synthetic-shared"})), None)).await?;
        assert_eq!(status, 201);
        // The shared credential, the tool list filtered, the protocol headers only.
        let (status, headers, body) = mcp(f, Some(&token), "tracker", &list()).await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["result"]["tools"], json!([{"name": "search"}]), "delete_issue is filtered out");
        // The remote's session id never reaches the runtime: it gets an opaque one instead.
        let opaque = headers.get("mcp-session-id").and_then(|value| value.to_str().ok()).ok_or("an opaque session")?;
        assert!(opaque.len() == 32 && opaque != "remote-session", "{opaque}");
        assert!(headers.get("set-cookie").is_none());
        assert_eq!(last_authorization(&remote).as_deref(), Some("Bearer synthetic-shared"));
        {
            let seen = remote.seen.lock().map_err(|_| "seen")?;
            let (forwarded, _) = seen.requests.last().ok_or("a request")?;
            assert!(forwarded.get("cookie").is_none(), "cookies never cross");
            assert!(forwarded.get("mcp-session-id").is_none(), "no session was named");
            assert!(!format!("{forwarded:?}").contains(&token), "the runtime token never reaches the remote");
        }
        // The user's own credential takes precedence.
        let (status, _) = call(f, Method::PUT, "/api/v1/connectors/tracker/credential", &cookie, (Some(json!({"value": "synthetic-own"})), None)).await?;
        assert_eq!(status, 204);
        let (status, _, body) = mcp(f, Some(&token), "tracker", &tool("search")).await?;
        assert_eq!((status, &body["result"]["content"][0]["text"]), (200, &json!("done")));
        assert_eq!(last_authorization(&remote).as_deref(), Some("Bearer synthetic-own"));
        // A notification-only body is accepted without an answer body.
        let (status, _, _) = mcp(f, Some(&token), "tracker", &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await?;
        assert_eq!(status, 200);
        // The header scheme sends the user's own credential in its header.
        let (status, _) = call(f, Method::PUT, "/api/v1/connectors/locked/credential", &cookie, (Some(json!({"value": "synthetic-key"})), None)).await?;
        assert_eq!(status, 204);
        let (status, _, _) = mcp(f, Some(&token), "locked", &list()).await?;
        assert_eq!(status, 200);
        {
            let seen = remote.seen.lock().map_err(|_| "seen")?;
            let (forwarded, _) = seen.requests.last().ok_or("a request")?;
            assert_eq!(forwarded.get("x-api-key").and_then(|value| value.to_str().ok()), Some("synthetic-key"));
            assert!(forwarded.get("authorization").is_none());
        }
        let (status, _) = call(f, Method::DELETE, "/api/v1/connectors/locked/credential", &cookie, (None, None)).await?;
        assert_eq!(status, 204);
        // A required credential that nobody has.
        let before = requests(&remote);
        let (status, _, body) = mcp(f, Some(&token), "locked", &list()).await?;
        assert_eq!((status, &body["error"]["code"]), (424, &json!("connector_credential_missing")));
        assert_eq!(requests(&remote), before, "nothing was sent");
        let metrics = metrics(f).await?;
        assert!(metrics.contains("tqa_mcp_proxy_requests_total{connector=\"tracker\",method=\"tools/list\",outcome=\"ok\"}"), "{metrics}");
        Ok(())
    })
    .await
}

// TEST: GW-I-069
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_069_the_proxy_refuses_callers_methods_and_tools_without_contacting_the_remote(
    pool: PgPool,
) -> TestResult {
    let (remote, address) = remote().await?;
    support::run_configured(pool, None, &seed(address), async |f| {
        let (_, cookie, token) = runtime(f, &["tracker", "off"]).await?;
        for (credential, expected) in [(None, 401), (Some("tqa-not-a-token"), 401)] {
            let (status, _, _) = mcp(f, credential, "tracker", &list()).await?;
            assert_eq!(status, expected);
        }
        // A session cookie is never a runtime credential.
        let response = f.client.post(f.url("/mcp/tracker")).header("cookie", &cookie).json(&list()).send().await?;
        assert_eq!(response.status(), 401);
        // Unknown, outside the conversation's set, or disabled: the same answer.
        for connector in ["absent", "other", "off"] {
            let (status, _, body) = mcp(f, Some(&token), connector, &list()).await?;
            assert_eq!((status, &body["error"]["code"]), (403, &json!("connector_not_allowed")), "{connector}");
        }
        let (status, _, body) = mcp(f, Some(&token), "tracker", &json!({"jsonrpc": "2.0", "id": 5, "method": "resources/list"})).await?;
        assert_eq!((status, &body["error"]["code"]), (200, &json!(-32601)));
        let (status, _, body) = mcp(f, Some(&token), "tracker", &tool("delete_issue")).await?;
        assert_eq!((status, &body["error"]["code"]), (200, &json!(-32602)));
        let (status, _, _) = mcp(f, Some(&token), "tracker", &json!({"not": "json-rpc"})).await?;
        assert_eq!(status, 400);
        let huge = json!({"jsonrpc": "2.0", "id": 1, "method": "ping", "params": {"pad": "x".repeat(1 << 20)}});
        let (status, _, _) = mcp(f, Some(&token), "tracker", &huge).await?;
        assert_eq!(status, 413);
        assert_eq!(requests(&remote), 0, "every refusal was decided at the proxy");
        Ok(())
    })
    .await
}

// TEST: GW-I-069
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_069_remote_failures_are_typed_and_never_passed_through(pool: PgPool) -> TestResult {
    let (remote, address) = remote().await?;
    support::run_configured(pool, None, &seed(address), async |f| {
        let (owner, _, token) = runtime(f, &["tracker"]).await?;
        for (mode, status, code) in [
            (Mode::ServerError, 502, "connector_unavailable"),
            (Mode::Redirect, 502, "connector_redirect"),
            (Mode::Huge, 502, "connector_answer_too_large"),
            (Mode::Slow, 504, "connector_timeout"),
            (Mode::Garbage, 502, "connector_unavailable"),
        ] {
            *remote.mode.lock().map_err(|_| "mode")? = Some(mode);
            let (actual, _, body) = mcp(f, Some(&token), "tracker", &list()).await?;
            assert_eq!(
                (actual, &body["error"]["code"]),
                (status, &json!(code)),
                "{code}"
            );
        }
        assert_eq!(
            *remote.redirected.lock().map_err(|_| "redirected")?,
            0,
            "the redirect was not followed"
        );
        // An empty answer passes with its status.
        *remote.mode.lock().map_err(|_| "mode")? = Some(Mode::Empty);
        let (status, _, _) = mcp(f, Some(&token), "tracker", &list()).await?;
        assert_eq!(status, 202);
        // A private CA that cannot be read fails closed.
        *remote.mode.lock().map_err(|_| "mode")? = None;
        let token = runtime_for(f, owner, &["private_ca"]).await?;
        let (status, _, body) = mcp(f, Some(&token), "private_ca", &list()).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (502, &json!("connector_unavailable"))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-069
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_069_a_remote_refusing_the_persons_own_credential_says_so(pool: PgPool) -> TestResult {
    let (remote, address) = remote().await?;
    support::run_configured(pool, None, &seed(address), async |f| {
        let (_, cookie, token) = runtime(f, &["tracker"]).await?;
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/admin/secrets/tracker-shared",
            &cookie,
            (Some(json!({"value": "synthetic-shared"})), None),
        )
        .await?;
        assert_eq!(status, 201);
        // The shared key refused: the person can't fix it, so it stays the remote's failure.
        *remote.mode.lock().map_err(|_| "mode")? = Some(Mode::Unauthorized);
        let (status, headers, body) = mcp(f, Some(&token), "tracker", &tool("search")).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (502, &json!("connector_unavailable")),
            "{body}"
        );
        assert_eq!(
            headers
                .get("x-tqa-gateway-error")
                .and_then(|value| value.to_str().ok()),
            Some("connector_unavailable")
        );
        // The person's own key refused, with a text body and with a JSON one: they're told it was their credential.
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/connectors/tracker/credential",
            &cookie,
            (Some(json!({"value": "synthetic-own"})), None),
        )
        .await?;
        assert_eq!(status, 204);
        for mode in [Mode::Unauthorized, Mode::Forbidden] {
            *remote.mode.lock().map_err(|_| "mode")? = Some(mode);
            let (status, headers, body) = mcp(f, Some(&token), "tracker", &tool("search")).await?;
            assert_eq!(
                (status, &body["error"]["code"]),
                (424, &json!("connector_credential_rejected")),
                "{body}"
            );
            assert_eq!(
                headers
                    .get("x-tqa-gateway-error")
                    .and_then(|value| value.to_str().ok()),
                Some("connector_credential_rejected"),
                "the runtime reads the code from the gateway's header"
            );
            assert!(
                !body.to_string().contains("synthetic-own"),
                "the key is never echoed"
            );
        }
        assert_eq!(
            last_authorization(&remote).as_deref(),
            Some("Bearer synthetic-own"),
            "the person's key was the one sent"
        );
        // The same key accepted again: the call goes through.
        *remote.mode.lock().map_err(|_| "mode")? = None;
        let (status, _, body) = mcp(f, Some(&token), "tracker", &tool("search")).await?;
        assert_eq!(
            (status, &body["result"]["content"][0]["text"]),
            (200, &json!("done"))
        );
        Ok(())
    })
    .await
}

struct Scripted(std::net::IpAddr);

#[async_trait::async_trait]
impl tqa_gateway::EgressResolver for Scripted {
    async fn resolve(&self, _: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
        Ok(vec![SocketAddr::new(self.0, port)])
    }
}

// TEST: SEC-024
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_024_connector_hosts_that_resolve_to_private_addresses_are_refused(
    pool: PgPool,
) -> TestResult {
    let (remote, address) = remote().await?;
    let named = format!(
        "[[connectors]]\nname='named'\nurl='http://tracker.test:{}/mcp'\ninsecure_http=true\n",
        address.port()
    );
    support::run_configured(
        pool,
        None,
        &format!("{}{named}", seed(address)),
        async |f| {
            let (_, _, token) = runtime(f, &["named", "tracker"]).await?;
            // A name answered with loopback or a private address is refused, even though the fake listens there.
            for private in [
                std::net::Ipv4Addr::LOCALHOST,
                std::net::Ipv4Addr::new(10, 0, 0, 5),
            ] {
                f.gateway
                    .set_connector_resolver(Arc::new(Scripted(private.into())));
                let (status, _, body) = mcp(f, Some(&token), "named", &list()).await?;
                assert_eq!(
                    (status, &body["error"]["code"]),
                    (502, &json!("connector_unavailable")),
                    "{private}"
                );
            }
            assert_eq!(requests(&remote), 0, "nothing reached the private address");
            // The URL naming the address itself is the administrator's explicit choice.
            let (status, _, _) = mcp(f, Some(&token), "tracker", &list()).await?;
            assert_eq!(status, 200);
            // An enabled connector outside the conversation's set is refused.
            let (status, _, body) = mcp(f, Some(&token), "other", &list()).await?;
            assert_eq!(
                (status, &body["error"]["code"]),
                (403, &json!("connector_not_allowed"))
            );
            Ok(())
        },
    )
    .await
}

async fn mcp_text(
    f: &Fixture,
    token: &str,
    connector: &str,
    body: &Value,
) -> Result<(u16, String, String)> {
    let response = f
        .client
        .post(f.url(&format!("/mcp/{connector}")))
        .bearer_auth(token)
        .json(body)
        .send()
        .await?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    Ok((status, content_type, response.text().await?))
}

// TEST: GW-I-070
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_070_event_stream_answers_are_relayed_whole_filtered_and_bounded(
    pool: PgPool,
) -> TestResult {
    let (remote, address) = remote().await?;
    support::run_configured(pool, None, &seed(address), async |f| {
        let (_, _, token) = runtime(f, &["tracker"]).await?;
        *remote.mode.lock().map_err(|_| "mode")? = Some(Mode::Stream);
        let (status, content_type, text) = mcp_text(f, &token, "tracker", &list()).await?;
        assert_eq!((status, content_type.as_str()), (200, "text/event-stream"));
        assert!(
            text.contains("event: message") && text.contains("\"search\""),
            "{text}"
        );
        assert!(
            !text.contains("delete_issue"),
            "the filtered tool is gone from the streamed list: {text}"
        );
        assert!(!text.contains("keep-alive"), "comments are not relayed");
        // An event larger than one message ends the stream with a JSON-RPC error instead of passing it on.
        *remote.mode.lock().map_err(|_| "mode")? = Some(Mode::HugeStream);
        let (status, _, text) = mcp_text(f, &token, "tracker", &list()).await?;
        assert_eq!(status, 200);
        assert!(
            text.contains("-32000") && text.len() < 4096,
            "cut off, not relayed: {} bytes",
            text.len()
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-070
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_070_a_fifth_call_in_flight_to_one_connector_is_busy(pool: PgPool) -> TestResult {
    let (remote, address) = remote().await?;
    let slow = seed(address).replace("call_timeout_seconds=1", "call_timeout_seconds=30");
    support::run_configured(pool, None, &slow, async |f| {
        let (_, _, token) = runtime(f, &["tracker"]).await?;
        *remote.mode.lock().map_err(|_| "mode")? = Some(Mode::Hold);
        let request = list();
        let held =
            futures::future::join_all((0..4).map(|_| mcp(f, Some(&token), "tracker", &request)));
        let fifth = async {
            tokio::time::timeout(support::DEADLINE, async {
                while requests(&remote) < 4 {
                    tokio::task::yield_now().await;
                }
            })
            .await?;
            let fifth = mcp(f, Some(&token), "tracker", &list()).await;
            remote.release.notify_waiters();
            fifth
        };
        let (held, fifth) = tokio::join!(held, fifth);
        let (status, _, body) = fifth?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (429, &json!("connector_busy"))
        );
        for answer in held {
            assert_eq!(answer?.0, 200);
        }
        assert_eq!(requests(&remote), 4, "the fifth never reached the remote");
        // The slots are free again.
        *remote.mode.lock().map_err(|_| "mode")? = None;
        assert_eq!(mcp(f, Some(&token), "tracker", &list()).await?.0, 200);
        Ok(())
    })
    .await
}

/// Sets the administrator's `mcp.calls_per_minute` override.
async fn set_rate(f: &Fixture, cookie: &str, value: u64) -> Result<()> {
    let (_, listing) = call(f, Method::GET, "/api/v1/admin/limits", cookie, (None, None)).await?;
    let version = listing["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["key"] == "mcp.calls_per_minute")
        .and_then(|item| item["version"].as_i64())
        .ok_or("the rate is listed")?;
    let (status, body) = call(
        f,
        Method::PUT,
        "/api/v1/admin/limits/mcp.calls_per_minute",
        cookie,
        (Some(json!({"value": value, "version": version})), None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(())
}

// TEST: GW-I-071
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_071_the_owners_call_rate_is_enforced_and_an_override_applies_without_restart(
    pool: PgPool,
) -> TestResult {
    let (remote, address) = remote().await?;
    let config = format!("{}[mcp]\ncalls_per_minute=2\n", seed(address));
    support::run_configured(pool, None, &config, async |f| {
        let (owner, cookie, token) = runtime(f, &["tracker"]).await?;
        assert_eq!(mcp(f, Some(&token), "tracker", &list()).await?.0, 200);
        assert_eq!(mcp(f, Some(&token), "tracker", &list()).await?.0, 200);
        let before = requests(&remote);
        let (status, headers, body) = mcp(f, Some(&token), "tracker", &list()).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (429, &json!("rate_limited")),
            "{body}"
        );
        let wait: u64 = headers
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok())
            .ok_or("Retry-After")?;
        assert!((1..=30).contains(&wait), "{wait}");
        assert_eq!(requests(&remote), before, "the remote was not contacted");
        // The bucket is the user's, not the conversation's.
        let other = runtime_for(f, owner, &["tracker"]).await?;
        assert_eq!(mcp(f, Some(&other), "tracker", &list()).await?.0, 429);
        // A call the proxy refuses itself costs nothing and is still answered.
        let (status, _, body) = mcp(f, Some(&token), "tracker", &tool("delete_issue")).await?;
        assert_eq!(status, 200);
        assert!(body["error"].is_object(), "{body}");
        // An administrator's raise applies from the next call: a hundred a second refill.
        set_rate(f, &cookie, 6000).await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(mcp(f, Some(&token), "tracker", &list()).await?.0, 200);
        // Lowered to 3: every message of a batch costs one.
        set_rate(f, &cookie, 3).await?;
        let pair = json!([list(), tool("search")]);
        assert_eq!(mcp(f, Some(&token), "tracker", &pair).await?.0, 200);
        let (status, _, body) = mcp(f, Some(&token), "tracker", &pair).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (429, &json!("rate_limited")),
            "{body}"
        );
        let metrics = metrics(f).await?;
        assert!(metrics.contains("outcome=\"rate_limited\""), "{metrics}");
        Ok(())
    })
    .await
}
