//! GW-I-041: the public origin proxies only credential-free pages, with streaming bodies.

#[path = "support/fixture.rs"]
mod fixture;
#[path = "support/web.rs"]
mod web;
use fixture::{DEADLINE, TestResult};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use sqlx::PgPool;
use std::sync::atomic::Ordering;

#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_041_api_only_pages_are_not_found(pool: PgPool) -> TestResult {
    fixture::run(pool, async |f| {
        assert_eq!(f.get("/login", "").await?.status(), 404);
        Ok(())
    })
    .await
}

#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_041_unavailable_web_returns_a_plain_502(pool: PgPool) -> TestResult {
    // Release an owned port so connection failure is immediate and deterministic.
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let config = format!("[web]\nupstream='http://{}'", listener.local_addr()?);
    drop(listener);
    fixture::run_configured(pool, None, &config, async |f| {
        let response = f.get("/login", "").await?;
        assert_eq!(response.status(), 502);
        assert!(
            response.headers()["content-type"]
                .to_str()?
                .starts_with("text/plain")
        );
        assert_eq!(
            response.text().await?.trim(),
            "The web interface is not available yet."
        );
        Ok(())
    })
    .await
}

#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_041_request_and_response_headers_are_isolated(pool: PgPool) -> TestResult {
    let upstream = web::Web::start().await?;
    fixture::run_configured(pool, None, &upstream.config(false), async |f| {
        let mut request = f.client.get(f.url("/c/example?view=compact"));
        for (key, value) in [
            ("cookie", "synthetic=session"),
            ("authorization", "Bearer synthetic"),
            ("proxy-authorization", "Basic synthetic"),
            ("proxy-custom", "drop"),
            ("x-forwarded-for", "192.0.2.1"),
            ("x-forwarded-host", "forged.example"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-custom", "drop"),
            ("connection", "x-private-request, keep-alive"),
            ("x-private-request", "drop"),
            ("rsc", "1"),
            ("next-router-state-tree", "fixture"),
            ("next-router-prefetch", "1"),
            ("next-url", "/c/example"),
        ] {
            request = request.header(key, value);
        }
        let response = request.send().await?;
        assert_eq!(response.status(), 203);
        for absent in ["set-cookie", "x-private-hop", "connection"] {
            assert!(!response.headers().contains_key(absent), "{absent}");
        }
        for (key, value) in [
            ("x-content-type-options", "nosniff"),
            ("referrer-policy", "same-origin"),
            ("x-frame-options", "DENY"),
            ("x-upstream", "preserved"),
            ("cache-control", "public, max-age=60"),
        ] {
            assert_eq!(response.headers()[key], value);
        }
        let body: Value = response.json().await?;
        assert_eq!(body["uri"], "/c/example?view=compact");
        let headers = &body["headers"];
        for absent in [
            "cookie",
            "authorization",
            "proxy-authorization",
            "proxy-custom",
            "x-forwarded-for",
            "x-forwarded-custom",
            "x-private-request",
            "connection",
        ] {
            assert!(headers.get(absent).is_none(), "{absent}: {headers}");
        }
        assert_eq!(headers["x-forwarded-proto"], "http");
        assert_eq!(
            headers["x-forwarded-host"],
            f.origin().trim_start_matches("http://")
        );
        for (key, value) in [
            ("rsc", "1"),
            ("next-router-state-tree", "fixture"),
            ("next-router-prefetch", "1"),
            ("next-url", "/c/example"),
        ] {
            assert_eq!(headers[key], value);
        }
        Ok(())
    })
    .await
}

#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_041_owned_namespaces_and_page_methods_never_reach_web(pool: PgPool) -> TestResult {
    let upstream = web::Web::start().await?;
    fixture::run_configured(pool, None, &upstream.config(false), async |f| {
        for path in [
            "/api",
            "/api/unknown",
            "/healthz",
            "/readyz",
            "/metrics",
            "/runtime",
            "/runtime/unknown",
            "/relay",
            "/relay/unknown",
        ] {
            let response = f.get(path, "").await?;
            assert_ne!(response.status(), 203, "{path}");
            if path.starts_with("/api") {
                assert_eq!(response.status(), 404);
                assert_eq!(
                    response.json::<Value>().await?["error"]["code"],
                    "not_found"
                );
            }
        }
        for method in [
            reqwest::Method::POST,
            reqwest::Method::PUT,
            reqwest::Method::DELETE,
            reqwest::Method::OPTIONS,
            reqwest::Method::PATCH,
        ] {
            assert_eq!(
                f.client.request(method, f.url("/")).send().await?.status(),
                405
            );
        }
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 0);
        let response = f.client.post(f.url("/api/unknown")).send().await?;
        assert_eq!(response.status(), 404);
        assert_eq!(
            response.json::<Value>().await?["error"]["code"],
            "not_found"
        );
        let response = f.client.head(f.url("/login")).send().await?;
        assert_eq!(response.status(), 203);
        assert!(response.bytes().await?.is_empty());
        Ok(())
    })
    .await
}

#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_041_page_body_limit_accepts_the_boundary_and_rejects_one_more_byte(
    pool: PgPool,
) -> TestResult {
    let upstream = web::Web::start().await?;
    fixture::run_configured(pool, None, &upstream.config(false), async |f| {
        for (size, status) in [(1024 * 1024, 203), (1024 * 1024 + 1, 413)] {
            let response = f
                .client
                .get(f.url("/login"))
                .body(vec![b' '; size])
                .send()
                .await?;
            assert_eq!(response.status(), status);
        }
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 1);
        Ok(())
    })
    .await
}

/// Sends one chunked `GET /login` (the HTTP client never streams a GET body) and returns the raw response.
async fn chunked_get(
    origin: &str,
    size: usize,
    count: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let address = origin.trim_start_matches("http://");
    let mut stream =
        tokio::time::timeout(DEADLINE, tokio::net::TcpStream::connect(address)).await??;
    let mut request = format!(
        "GET /login HTTP/1.1\r\nhost: {address}\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n"
    )
    .into_bytes();
    for _ in 0..count {
        request.extend_from_slice(format!("{size:x}\r\n").as_bytes());
        request.extend(std::iter::repeat_n(b' ', size));
        request.extend_from_slice(b"\r\n");
    }
    request.extend_from_slice(b"0\r\n\r\n");
    tokio::time::timeout(DEADLINE, stream.write_all(&request)).await??;
    let mut response = Vec::new();
    tokio::time::timeout(DEADLINE, stream.read_to_end(&mut response)).await??;
    Ok(String::from_utf8(response)?)
}

#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_041_page_request_bodies_are_read_and_never_forwarded(pool: PgPool) -> TestResult {
    let upstream = web::Web::start().await?;
    fixture::run_configured(pool, None, &upstream.config(false), async |f| {
        let response = f
            .client
            .get(f.url("/login"))
            .body(vec![b' '; 4096])
            .send()
            .await?;
        assert_eq!(response.status(), 203);
        let sized: Value = response.json().await?;
        let chunked = chunked_get(f.origin(), 4096, 2).await?;
        assert!(chunked.starts_with("HTTP/1.1 203"), "{chunked}");
        // The JSON body, whatever the response framing.
        let json = chunked.find('{').zip(chunked.rfind('}')).ok_or("no body")?;
        let chunked: Value = serde_json::from_str(&chunked[json.0..=json.1])?;
        for body in [sized, chunked] {
            for absent in ["content-length", "transfer-encoding"] {
                assert!(body["headers"].get(absent).is_none(), "{absent}: {body}");
            }
        }
        assert_eq!(upstream.requests.load(Ordering::SeqCst), 2);
        Ok(())
    })
    .await
}

#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_041_streaming_returns_the_first_chunk_before_upstream_finishes(
    pool: PgPool,
) -> TestResult {
    let upstream = web::Web::start().await?;
    fixture::run_configured(pool, None, &upstream.config(false), async |f| {
        let response = f.get("/stream", "").await?;
        let mut stream = response.bytes_stream();
        let chunk = tokio::time::timeout(DEADLINE, stream.next())
            .await?
            .ok_or("no chunk")??;
        assert_eq!(chunk.as_ref(), b"first chunk");
        Ok(())
    })
    .await
}

#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_041_hot_reload_is_disabled_by_default(pool: PgPool) -> TestResult {
    let upstream = web::Web::start().await?;
    fixture::run_configured(pool, None, &upstream.config(false), async |f| {
        let url = format!("{}/_next/hmr", f.origin().replacen("http", "ws", 1));
        assert!(
            tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(url))
                .await?
                .is_err()
        );
        Ok(())
    })
    .await
}

#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_041_hot_reload_can_be_enabled_explicitly(pool: PgPool) -> TestResult {
    let upstream = web::Web::start().await?;
    fixture::run_configured(pool, None, &upstream.config(true), async |f| {
        let url = format!("{}/_next/hmr", f.origin().replacen("http", "ws", 1));
        let (mut socket, _) =
            tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(url)).await??;
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                "reload".into(),
            ))
            .await?;
        assert_eq!(
            tokio::time::timeout(DEADLINE, socket.next())
                .await?
                .ok_or("closed")??
                .into_text()?,
            "reload"
        );
        socket.close(None).await?;
        Ok(())
    })
    .await
}

// TEST: GW-I-041
#[sqlx::test(migrator = "tqa_gateway::MIGRATOR")]
#[ignore = "requires TQA_DATABASE_URL and DATABASE_URL"]
async fn gw_i_041_unmatched_mcp_paths_answer_json_not_the_page_fallback(
    pool: PgPool,
) -> TestResult {
    // A runtime that builds an MCP URL wrong must get an API answer it can read, never the web application's 405 or
    // page (seen in SEC-013's driver: a gateway URL ending in "/" made "//mcp/x").
    let upstream = web::Web::start().await?;
    fixture::run_configured(pool, None, &upstream.config(false), async |f| {
        for path in [
            "/mcp",
            "/mcp/",
            "/mcp/tracker/extra",
            "//mcp/tracker",
            "//api/unknown",
        ] {
            for method in [reqwest::Method::GET, reqwest::Method::POST] {
                let response = f.client.request(method.clone(), f.url(path)).send().await?;
                assert_eq!(response.status(), 404, "{method} {path}");
                assert_eq!(
                    response.json::<Value>().await?["error"]["code"],
                    "not_found",
                    "{method} {path}"
                );
            }
        }
        assert_eq!(
            upstream.requests.load(Ordering::SeqCst),
            0,
            "nothing reached the web application"
        );
        Ok(())
    })
    .await
}
