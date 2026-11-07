//! GW-I-070 (ADR-0031 decisions 4 and 6): MCP sessions through the proxy. The runtime only ever sees an opaque session
//! id bound to its conversation, the connector and the credential source; another conversation's id, a changed
//! credential or a closed session is 404 (MCP clients then initialise again); the session's server stream is relayed,
//! one at a time; DELETE ends it at the remote; and a call the runtime abandons is cancelled at the remote.
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

/// A remote MCP server with sessions: `initialize` opens one, `GET` streams one event and holds, `DELETE` closes, and a
/// `tools/call` for `slow` waits for the test.
#[derive(Default)]
struct Remote {
    seen: Mutex<Vec<(String, Option<String>, Value)>>,
    opened: Mutex<u32>,
    release: tokio::sync::Notify,
}

fn session(headers: &HeaderMap) -> Option<String> {
    headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

async fn post_answer(
    State(remote): State<Arc<Remote>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let message: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if let Ok(mut seen) = remote.seen.lock() {
        seen.push((format!("POST {method}"), session(&headers), message.clone()));
    }
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    if method == "initialize" {
        let number = remote.opened.lock().map_or(0, |mut opened| {
            *opened += 1;
            *opened
        });
        return (
            [("mcp-session-id", format!("remote-{number}"))],
            axum::Json(
                json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": "2025-11-25"}}),
            ),
        )
            .into_response();
    }
    if message.pointer("/params/name") == Some(&json!("slow")) {
        remote.release.notified().await;
    }
    if method.starts_with("notifications/") {
        return StatusCode::ACCEPTED.into_response();
    }
    axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": {"tools": []}})).into_response()
}

async fn get_stream(State(remote): State<Arc<Remote>>, headers: HeaderMap) -> Response {
    if let Ok(mut seen) = remote.seen.lock() {
        seen.push(("GET".into(), session(&headers), Value::Null));
    }
    let first = futures::stream::once(async {
        Ok::<_, std::io::Error>(Bytes::from(
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/tools/list_changed\"}\n\n",
        ))
    });
    let held = futures::stream::once(async move {
        remote.release.notified().await;
        Ok::<_, std::io::Error>(Bytes::new())
    });
    let body = axum::body::Body::from_stream(futures::StreamExt::chain(first, held));
    ([("content-type", "text/event-stream")], body).into_response()
}

async fn delete_session(State(remote): State<Arc<Remote>>, headers: HeaderMap) -> StatusCode {
    if let Ok(mut seen) = remote.seen.lock() {
        seen.push(("DELETE".into(), session(&headers), Value::Null));
    }
    StatusCode::OK
}

async fn remote() -> Result<(Arc<Remote>, SocketAddr)> {
    let remote = Arc::new(Remote::default());
    let app = Router::new()
        .route(
            "/mcp",
            post(post_answer).get(get_stream).delete(delete_session),
        )
        .with_state(remote.clone());
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok((remote, address))
}

fn seed(address: SocketAddr) -> String {
    format!(
        "[[connectors]]\nname='tracker'\nurl='http://{address}/mcp'\ninsecure_http=true\nauth={{scheme='bearer'}}\n\
         secret_ref='secrets/tracker-shared'\ncall_timeout_seconds=30\n"
    )
}

/// A conversation of `owner` with the tracker in its set, and its runtime token.
async fn runtime(f: &Fixture, owner: uuid::Uuid) -> Result<String> {
    let conversation = f.conversation(owner).await?;
    let id = uuid::Uuid::parse_str(conversation.id.as_str())?;
    sqlx::query("UPDATE conversations SET connectors='{tracker}' WHERE id=$1")
        .bind(id)
        .execute(&f.pool)
        .await?;
    let sandbox = format!("sessions-{}", &id.simple().to_string()[20..]);
    Ok(f.gateway
        .runtime_tokens()
        .mint(
            &conversation.id,
            &sandbox.parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?
        .expose_secret()
        .to_owned())
}

async fn send(
    f: &Fixture,
    method: Method,
    token: &str,
    session: Option<&str>,
    body: Option<&Value>,
) -> Result<reqwest::Response> {
    let mut request = f
        .client
        .request(method, f.url("/mcp/tracker"))
        .bearer_auth(token);
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    if let Some(body) = body {
        request = request.json(body);
    }
    Ok(request.send().await?)
}

fn initialize() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
}

fn list() -> Value {
    json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
}

fn seen(remote: &Remote) -> Vec<(String, Option<String>, Value)> {
    remote
        .seen
        .lock()
        .map(|seen| seen.clone())
        .unwrap_or_default()
}

async fn open(f: &Fixture, token: &str) -> Result<String> {
    let response = send(f, Method::POST, token, None, Some(&initialize())).await?;
    assert_eq!(response.status(), 200);
    Ok(response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .ok_or("a session id")?
        .to_owned())
}

async fn setup(f: &Fixture) -> Result<(String, uuid::Uuid)> {
    let (owner, cookie) = f.admin().await?;
    let (status, _) = call(
        f,
        Method::PUT,
        "/api/v1/admin/secrets/tracker-shared",
        &cookie,
        (Some(json!({"value": "synthetic-shared"})), None),
    )
    .await?;
    assert_eq!(status, 201);
    Ok((cookie, owner.id))
}

// TEST: GW-I-070
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_070_a_session_is_opaque_and_bound_to_its_conversation_and_credential(
    pool: PgPool,
) -> TestResult {
    let (remote, address) = remote().await?;
    support::run_configured(pool, None, &seed(address), async |f| {
        let (cookie, owner) = setup(f).await?;
        let token = runtime(f, owner).await?;
        let opaque = open(f, &token).await?;
        assert!(
            opaque.len() == 32 && !opaque.starts_with("remote-"),
            "{opaque}"
        );
        let answer = send(f, Method::POST, &token, Some(&opaque), Some(&list())).await?;
        assert_eq!(answer.status(), 200);
        assert_eq!(
            answer
                .headers()
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok()),
            Some(opaque.as_str())
        );
        assert_eq!(
            seen(&remote).last().map(|request| request.1.clone()),
            Some(Some("remote-1".into())),
            "the remote id is used upstream"
        );
        // Another conversation cannot use it, and the remote is not contacted.
        let other = runtime(f, owner).await?;
        let before = seen(&remote).len();
        assert_eq!(
            send(f, Method::POST, &other, Some(&opaque), Some(&list()))
                .await?
                .status(),
            404
        );
        assert_eq!(
            send(
                f,
                Method::POST,
                &token,
                Some("0123456789abcdef0123456789abcdef"),
                Some(&list())
            )
            .await?
            .status(),
            404
        );
        assert_eq!(seen(&remote).len(), before);
        assert_eq!(
            send(f, Method::POST, &token, Some(&opaque), Some(&list()))
                .await?
                .status(),
            200,
            "still the owner's"
        );
        // The user adds their own credential: the session opened with the shared one ends; a new one opens.
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/connectors/tracker/credential",
            &cookie,
            (Some(json!({"value": "synthetic-own"})), None),
        )
        .await?;
        assert_eq!(status, 204);
        assert_eq!(
            send(f, Method::POST, &token, Some(&opaque), Some(&list()))
                .await?
                .status(),
            404
        );
        let renewed = open(f, &token).await?;
        assert_ne!(renewed, opaque);
        assert_eq!(
            send(f, Method::POST, &token, Some(&renewed), Some(&list()))
                .await?
                .status(),
            200
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-070
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_070_the_server_stream_is_relayed_once_and_delete_ends_the_session(
    pool: PgPool,
) -> TestResult {
    let (remote, address) = remote().await?;
    support::run_configured(pool, None, &seed(address), async |f| {
        let (_, owner) = setup(f).await?;
        let token = runtime(f, owner).await?;
        assert_eq!(
            send(f, Method::GET, &token, None, None).await?.status(),
            400,
            "a stream needs a session"
        );
        let opaque = open(f, &token).await?;
        let mut stream = send(f, Method::GET, &token, Some(&opaque), None).await?;
        assert_eq!(stream.status(), 200);
        let first = tokio::time::timeout(support::DEADLINE, stream.chunk())
            .await??
            .ok_or("an event")?;
        assert!(String::from_utf8_lossy(&first).contains("list_changed"));
        assert_eq!(
            send(f, Method::GET, &token, Some(&opaque), None)
                .await?
                .status(),
            409,
            "one stream per session"
        );
        assert_eq!(
            seen(&remote)
                .iter()
                .filter(|request| request.0 == "GET")
                .count(),
            1
        );
        // DELETE ends the session at the remote; the id is gone.
        let deleted = send(f, Method::DELETE, &token, Some(&opaque), None).await?;
        assert_eq!(deleted.status(), 200);
        assert!(
            seen(&remote)
                .iter()
                .any(|request| request.0 == "DELETE" && request.1.as_deref() == Some("remote-1"))
        );
        assert_eq!(
            send(f, Method::POST, &token, Some(&opaque), Some(&list()))
                .await?
                .status(),
            404
        );
        remote.release.notify_waiters();
        drop(stream);
        Ok(())
    })
    .await
}

// TEST: GW-I-070
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_070_a_call_the_runtime_abandons_is_cancelled_at_the_remote(
    pool: PgPool,
) -> TestResult {
    let (remote, address) = remote().await?;
    support::run_configured(pool, None, &seed(address), async |f| {
        let (_, owner) = setup(f).await?;
        let token = runtime(f, owner).await?;
        let opaque = open(f, &token).await?;
        let slow = json!({"jsonrpc": "2.0", "id": 42, "method": "tools/call", "params": {"name": "slow", "arguments": {}}});
        // The runtime gives up after a moment (a cancelled run): its request is dropped.
        let abandoned = tokio::time::timeout(Duration::from_millis(500), send(f, Method::POST, &token, Some(&opaque), Some(&slow))).await;
        assert!(abandoned.is_err(), "the call was still waiting");
        let cancelled = tokio::time::timeout(support::DEADLINE, async {
            loop {
                if let Some(request) = seen(&remote).into_iter().find(|request| request.0 == "POST notifications/cancelled") {
                    return request;
                }
                tokio::task::yield_now().await;
            }
        })
        .await?;
        assert_eq!(cancelled.1.as_deref(), Some("remote-1"), "sent within the session");
        assert_eq!(cancelled.2["params"]["requestId"], 42);
        remote.release.notify_waiters();
        Ok(())
    })
    .await
}
