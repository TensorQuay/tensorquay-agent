//! Gate 3 security evidence on the gateway's HTTP surface: SEC-004, SEC-005, SEC-009, SEC-012 (HTTP half), and
//! NUL in stored text fields (answers-056). SEC-026: runtime workspace replies never reach clients (ADR-0030).
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

/// A documentation address (RFC 5737): a token bound to it is never presented from this test's loopback peer.
const OTHER_SANDBOX: std::net::Ipv4Addr = std::net::Ipv4Addr::new(192, 0, 2, 10);

async fn error_code(
    response: reqwest::Response,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    let status = response.status().as_u16();
    let body: Value = response.json().await?;
    Ok((status, body["error"].clone()))
}

/// Every runtime-token endpoint, as `(name, status)` for one bearer token.
async fn runtime_statuses(
    f: &Fixture,
    token: &str,
) -> Result<Vec<(&'static str, u16)>, Box<dyn std::error::Error>> {
    let relay_chat = f
        .client
        .post(f.url("/relay/r/local/chat/completions"))
        .bearer_auth(token)
        .json(&json!({"model":"configured-model","messages":[{"role":"user","content":"hi"}]}))
        .send()
        .await?
        .status()
        .as_u16();
    let relay_models = f
        .client
        .get(f.url("/relay/r/local/models"))
        .bearer_auth(token)
        .send()
        .await?
        .status()
        .as_u16();
    let history = f
        .client
        .get(f.url("/runtime/history?after=0&limit=10"))
        .bearer_auth(token)
        .send()
        .await?
        .status()
        .as_u16();
    // A WebSocket handshake: the link is refused before any upgrade.
    let link = f
        .client
        .get(f.url("/runtime/link"))
        .bearer_auth(token)
        .header("connection", "upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await?
        .status()
        .as_u16();
    Ok(vec![
        ("relay chat", relay_chat),
        ("relay models", relay_models),
        ("history", history),
        ("link", link),
    ])
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_004_a_runtime_token_works_only_for_its_own_sandbox_and_never_after_revocation(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, _) = f.admin().await?;
        let a = f.conversation(user.id).await?;
        let b = f.conversation(user.id).await?;
        f.gateway
            .events()
            .append(&a.id, support::frame(1, "a's history"))
            .await?;
        f.gateway
            .events()
            .append(&b.id, support::frame(1, "b's history"))
            .await?;
        let tokens = f.gateway.runtime_tokens();
        // B's sandbox token, presented from another sandbox's address (a replayed or stolen token).
        let foreign_sandbox = "sandbox-b".parse()?;
        let replayed = tokens
            .mint(&b.id, &foreign_sandbox, OTHER_SANDBOX.into())
            .await?;
        for (endpoint, status) in runtime_statuses(f, replayed.expose_secret()).await? {
            assert_eq!(
                status, 401,
                "a token replayed from another sandbox: {endpoint}"
            );
        }
        // A's own token reads only A: its history never contains B's frames, and it cannot name B.
        let own_sandbox = "sandbox-a".parse()?;
        let own = tokens
            .mint(&a.id, &own_sandbox, std::net::Ipv4Addr::LOCALHOST.into())
            .await?;
        let page: Value = f
            .client
            .get(f.url("/runtime/history?after=0&limit=10"))
            .bearer_auth(own.expose_secret())
            .send()
            .await?
            .json()
            .await?;
        assert_eq!(page["items"].as_array().map(Vec::len), Some(1));
        assert_eq!(page["items"][0]["event"]["text"], "a's history");
        let naming_b = f
            .client
            .get(f.url(&format!(
                "/runtime/history?after=0&limit=10&conversation_id={}",
                b.id
            )))
            .bearer_auth(own.expose_secret())
            .send()
            .await?;
        assert_eq!(
            naming_b.status(),
            400,
            "a token never selects another conversation"
        );
        // Revoked: every endpoint refuses the same token that worked a moment ago.
        tokens.revoke(&own_sandbox).await?;
        for (endpoint, status) in runtime_statuses(f, own.expose_secret()).await? {
            assert_eq!(status, 401, "a revoked token: {endpoint}");
        }
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_005_another_user_is_refused_everywhere_and_nothing_changes(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, owner_cookie) = f.admin().await?;
        let (_, other) = f.member("other@example.invalid").await?;
        let conversation = f.conversation(owner.id).await?;
        let base = format!("/api/v1/conversations/{}", conversation.id);
        let origin = f.origin().to_owned();
        let request = |method: Method, path: String| {
            f.client
                .request(method, f.url(&path))
                .header("origin", &origin)
                .header("cookie", &other)
                .header("idempotency-key", format!("other-{path}"))
        };
        let attempts = [
            ("get", request(Method::GET, base.clone())),
            (
                "history",
                request(Method::GET, format!("{base}/events?after=0&limit=10")),
            ),
            (
                "stream",
                request(Method::GET, format!("{base}/events?stream=1&protocol=1")),
            ),
            (
                "message",
                request(Method::POST, format!("{base}/messages")).json(&json!({"text":"hijack"})),
            ),
            (
                "cancel",
                request(Method::POST, format!("{base}/runs/r-1/cancel")).json(&json!({})),
            ),
            (
                "patch",
                request(Method::PATCH, base.clone()).json(&json!({"title":"taken"})),
            ),
            (
                "environment",
                request(Method::PUT, format!("{base}/environment"))
                    .json(&json!({"template":"minimal"})),
            ),
            (
                "files",
                request(Method::GET, format!("{base}/files")).query(&[("path", ".")]),
            ),
            (
                "file content",
                request(Method::GET, format!("{base}/files/content"))
                    .query(&[("path", "README.md")]),
            ),
            (
                "file download",
                request(Method::GET, format!("{base}/files/download"))
                    .query(&[("path", "README.md")]),
            ),
            (
                "upload",
                request(Method::POST, format!("{base}/uploads"))
                    .query(&[("name", "planted.txt")])
                    .body("planted"),
            ),
            ("delete", request(Method::DELETE, base.clone())),
        ];
        for (name, attempt) in attempts {
            let (status, error) = error_code(attempt.send().await?).await?;
            assert_eq!(
                (status, error["code"].as_str()),
                (403, Some("forbidden")),
                "{name}"
            );
        }
        // Nothing changed: same title, still active, nothing queued, nothing cached for the other user.
        let mine: Value = f.get(&base, &owner_cookie).await?.json().await?;
        assert_eq!(
            (mine["title"].as_str(), mine["state"].as_str()),
            (Some("Fixture"), Some("active"))
        );
        let traces: i64 = sqlx::query_scalar("SELECT count(*) FROM idempotency")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(traces, 0);
        assert_eq!(
            f.backend.creates.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no sandbox was started"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_009_login_attempts_beyond_ten_a_minute_are_refused_even_with_the_right_password(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        // The fixture's own login is the first attempt from this address in the window.
        f.admin().await?;
        let mut statuses = Vec::new();
        for _ in 0..50 {
            let response = f
                .post(
                    "/api/v1/auth/login",
                    &json!({"email":"admin@example.invalid","password":"not the password"}),
                )
                .await?;
            statuses.push(response.status().as_u16());
        }
        assert_eq!(
            statuses[..9],
            [401; 9],
            "nine more attempts are allowed in the window"
        );
        assert!(
            statuses[9..].iter().all(|status| *status == 429),
            "{statuses:?}"
        );
        let right = f
            .post(
                "/api/v1/auth/login",
                &json!({"email":"admin@example.invalid","password":f.password()}),
            )
            .await?;
        // Locked out for the window, said as the address's rate limit with the window's real wait (GW-I-001), never
        // more than the minute.
        let wait = right
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        let (status, error) = error_code(right).await?;
        assert_eq!(
            (status, error["code"].as_str()),
            (429, Some("rate_limited")),
            "locked out for the window"
        );
        assert!(
            wait.is_some_and(|seconds| (1..=60).contains(&seconds)),
            "Retry-After {wait:?}"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_012_an_oversized_body_is_refused_before_parsing_and_the_exact_limit_is_accepted(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let path = format!("/api/v1/conversations/{}/messages", conversation.id);
        // The default `limits.body_bytes` is 1 MiB; whitespace pads a valid message to an exact size.
        let limit = 1024 * 1024;
        let body = |size: usize| {
            let mut bytes = br#"{"text":"within the limit"}"#.to_vec();
            bytes.resize(size, b' ');
            bytes
        };
        let send = |bytes: Vec<u8>, key: &str| {
            f.client
                .post(f.url(&path))
                .header("origin", f.origin())
                .header("cookie", &cookie)
                .header("idempotency-key", key)
                .header("content-type", "application/json")
                .body(bytes)
                .send()
        };
        assert_eq!(
            send(body(limit), "exact").await?.status(),
            202,
            "exactly the limit is accepted"
        );
        for (size, key) in [
            (limit + 1, "one-over"),
            (50 * 1024 * 1024, "fifty-megabytes"),
        ] {
            let (status, error) = error_code(send(body(size), key).await?).await?;
            assert_eq!(
                (status, error["code"].as_str(), error["retryable"].as_bool()),
                (413, Some("body_too_large"), Some(false)),
                "{size} bytes"
            );
        }
        let cached: i64 = sqlx::query_scalar("SELECT count(*) FROM idempotency")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(cached, 1, "only the accepted message was recorded");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn nul_in_a_stored_text_field_is_an_invalid_request_not_a_store_failure(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let created = f
            .action(
                "/api/v1/conversations",
                &cookie,
                Some("nul-title"),
                &json!({"title":"before\u{0}after","model_route":"local"}),
            )
            .await?;
        let conversation = f.conversation(user.id).await?;
        let patched = f
            .client
            .patch(f.url(&format!("/api/v1/conversations/{}", conversation.id)))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .header("idempotency-key", "nul-rename")
            .json(&json!({"title":"before\u{0}after"}))
            .send()
            .await?;
        let login = f
            .post(
                "/api/v1/auth/login",
                &json!({"email":"admin\u{0}@example.invalid","password":f.password()}),
            )
            .await?;
        for (name, response) in [
            ("create", created),
            ("rename", patched),
            ("login email", login),
        ] {
            let (status, error) = error_code(response).await?;
            assert_eq!(
                (status, error["code"].as_str(), error["retryable"].as_bool()),
                (400, Some("invalid_request"), Some(false)),
                "{name}"
            );
        }
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_026_runtime_workspace_replies_never_reach_clients_or_the_store(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let mut stream = f.stream(id, &cookie, 0).await?;
        // A full chunk of data: the largest reply a runtime can send within one frame.
        let chunk = "AAAA".repeat(tqa_types::WORKSPACE_CHUNK_BYTES / 3);
        let unasked = [
            json!({"type": "workspace.reply", "request_id": 1, "part": 0, "last": true,
                "result": {"kind": "listing", "items": [{"name": "secret.txt", "kind": "file", "size": 1, "modified": 0}]}}),
            json!({"type": "workspace.reply", "request_id": u64::MAX, "part": 7, "last": false,
                "result": {"kind": "content", "size": 1, "modified": 0, "inode": 1, "data": chunk, "eof": false}}),
            json!({"type": "workspace.reply", "request_id": 0, "part": 0, "last": true,
                "result": {"kind": "error", "code": "internal", "message": "for a client", "retryable": false}}),
        ];
        for reply in unasked {
            let frame = tqa_types::ServerFrame::parse(reply.to_string().as_bytes())?;
            runtime.send(&frame).await?;
        }
        runtime.send(&support::frame(1, "after the replies")).await?;
        // Everything the client sees up to the next durable frame: no reply among it.
        loop {
            let (seq, frame) = stream.next().await?;
            assert!(
                !matches!(frame, tqa_types::ServerFrame::WorkspaceReply(_)),
                "a workspace reply reached a client: {frame:?}"
            );
            if seq.is_some() {
                break;
            }
        }
        assert_eq!(f.gateway.events().read(id, 0.into(), 10).await?.len(), 1);
        // The link stays up: an unasked reply is dropped, not a protocol error.
        runtime.roundtrip().await?;
        Ok(())
    })
    .await
}
