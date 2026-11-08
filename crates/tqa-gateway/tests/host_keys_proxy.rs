//! GW-I-166 (ADR-0037 §4, §5, §6), without the stack: the proxy opens TLS only for a host the caller's running run holds a
//! key for, with the install CA's leaf for exactly that host, and verified TLS upstream; it swaps a placeholder in its
//! one accepted form for the value, asks for an uncompressed answer and refuses a compressed one; it refuses a
//! placeholder anywhere else (a request body included) and a request naming another host, so nothing of it reaches the
//! host; an unkeyed host's tunnel is untouched; an upstream certificate that fails verification is refused, never a
//! fallback; and a placeholder over plain http is refused.
//! GW-I-176 (ADR-0037 Amendment 2): a key for an exact IPv4 or IPv6 address is used through that address's own allow
//! rule, with an IP-SAN leaf and upstream verified by the address; an address without its rule, or another address on
//! its tunnel, never gets it.
//! GW-I-175 (ADR-0037's G9a): a keyed host behind an internal certificate authority is reached when `egress.ca_file`
//! names the authority, on the release build's code path (no test hook).
#[path = "support/keyed.rs"]
mod keyed;
#[path = "support/fixture.rs"]
mod support;

use base64::Engine as _;
use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use keyed::*;
use reqwest::Method;
use sqlx::PgPool;
use std::net::SocketAddr;
use support::TestResult;
use support::call;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// TEST: GW-I-166
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_166_a_placeholder_in_its_place_is_swapped_and_the_answer_scrubbed(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let mut client = keyed_client(run.proxy, HOST, run.install.clone()).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        assert_eq!(
            send(&mut client, get(HOST, "/v1/me", Some(&bearer))?).await?,
            (200, "ok".to_owned())
        );
        assert_eq!(
            send(
                &mut client,
                get(HOST, "/v1/open", Some("Bearer someone-elses"))?
            )
            .await?,
            (200, "ok".to_owned())
        );
        // An answer that sends the key back carries the placeholder in its place, in a header and the body.
        let (header, body) = echo(&mut client, get(HOST, "/echo", Some(&bearer))?).await?;
        assert_eq!(
            (header.as_str(), body),
            (bearer.as_str(), format!("you sent {bearer}"))
        );
        let seen = hosts.seen.0.lock().map_err(|_| "lock")?.clone();
        let swapped = Sent {
            target: "/v1/me".into(),
            path: "/v1/me".into(),
            authorization: format!("Bearer {VALUE}"),
            encoding: "identity".into(),
            body: String::new(),
        };
        assert_eq!(seen[0], swapped);
        assert_eq!(
            (seen[1].authorization.as_str(), seen[1].encoding.as_str()),
            ("Bearer someone-elses", "gzip"),
            "unchanged"
        );
        // Each swap is counted (guardrails §1.12).
        assert_eq!(uses(f, "ok").await?, 2);
        Ok(())
    })
    .await
}

// TEST: GW-I-166
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_166_a_placeholder_anywhere_else_never_reaches_the_host(pool: PgPool) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let mut client = keyed_client(run.proxy, HOST, run.install.clone()).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        let (status, words) = send(
            &mut client,
            get(HOST, &format!("/v1/me?token={}", run.placeholder), None)?,
        )
        .await?;
        assert!(
            status == 403 && words.contains("Authorization header"),
            "{status} {words}"
        );
        let (status, words) =
            send(&mut client, get("other.test", "/v1/me", Some(&bearer))?).await?;
        assert!(status == 403 && words.contains(HOST), "{status} {words}");
        let (status, _) = send(&mut client, get(HOST, "/gz", Some(&bearer))?).await?;
        assert_eq!(
            status, 502,
            "a compressed answer to a keyed request is refused"
        );
        let upload = Request::post("/upload")
            .header("host", HOST)
            .body(Full::new(Bytes::from(format!(
                "data {} more",
                run.placeholder
            ))))?;
        assert!(
            send(&mut client, upload)
                .await
                .map_or(true, |(status, _)| status != 200),
            "a body holding one never goes"
        );
        let seen = hosts.seen.0.lock().map_err(|_| "lock")?.clone();
        assert!(
            seen.iter().all(|sent| sent.path != "/upload"
                && !sent.path.contains("tqa_ph_")
                && !sent.body.contains("tqa_ph_")),
            "{seen:?}"
        );
        // Each misplaced placeholder, the query's and the body's, is a counted refusal (guardrails §1.12).
        assert_eq!(uses(f, "wrong_position").await?, 2);
        Ok(())
    })
    .await
}

// TEST: GW-I-166
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_166_a_placeholder_never_goes_over_plain_http(pool: PgPool) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        // A forwarded http request with a placeholder: refused, a key never goes over it.
        let mut socket = tokio::net::TcpStream::connect(run.proxy).await?;
        let plain = format!(
            "GET http://{HOST}:443/v1 HTTP/1.1\r\nHost: {HOST}\r\nAuthorization: {bearer}\r\n\r\n"
        );
        socket.write_all(plain.as_bytes()).await?;
        let mut answer = Vec::new();
        tokio::time::timeout(support::DEADLINE, socket.read_to_end(&mut answer)).await??;
        assert!(
            String::from_utf8_lossy(&answer).starts_with("HTTP/1.1 403"),
            "{}",
            String::from_utf8_lossy(&answer)
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-166
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_166_only_keyed_hosts_are_opened_and_never_by_a_fallback(pool: PgPool) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        // An unkeyed host's tunnel is untouched: the client sees the host's own certificate, not the install CA's.
        let mut real = rustls::RootCertStore::empty();
        real.add(rustls::pki_types::CertificateDer::from(hosts.ca.clone()))?;
        let mut plain = keyed_client(run.proxy, "plain.test", real).await?;
        assert_eq!(
            send(&mut plain, get("plain.test", "/", None)?).await?,
            (200, "ok".to_owned())
        );
        assert!(
            keyed_client(run.proxy, "plain.test", run.install.clone())
                .await
                .is_err(),
            "the install CA never answers it"
        );
        // A keyed host whose certificate fails verification: refused in words, never a fallback.
        let mut bad = keyed_client(run.proxy, "bad.keyed.test", run.install.clone()).await?;
        let (status, words) = send(
            &mut bad,
            get(
                "bad.keyed.test",
                "/",
                Some(&format!("Bearer {}", run.placeholder)),
            )?,
        )
        .await?;
        assert!(
            status == 502 && words.contains("couldn't be verified"),
            "{status} {words}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-166
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_166_a_keyed_host_that_never_answers_is_refused_within_its_bound(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    // keyed_answer_seconds = 1: the real wait is a second, bounded by DEADLINE (a real socket, no fake clock).
    support::run_configured(pool, None, &config(1), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let mut client = keyed_client(run.proxy, HOST, run.install.clone()).await?;
        let started = std::time::Instant::now();
        let request = get(HOST, "/hang", Some(&format!("Bearer {}", run.placeholder)))?;
        let (status, _) =
            tokio::time::timeout(support::DEADLINE, send(&mut client, request)).await??;
        assert_eq!(status, 502);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        Ok(())
    })
    .await
}

/// `der` as a PEM file in `directory`, for a `ca_file` setting.
fn pem_file(directory: &std::path::Path, der: &[u8]) -> Result<std::path::PathBuf, Error> {
    let body = base64::engine::general_purpose::STANDARD.encode(der);
    let lines: Vec<&str> = body
        .as_bytes()
        .chunks(64)
        .map(std::str::from_utf8)
        .collect::<Result<_, _>>()?;
    // The armour is built here, so no certificate-shaped text sits in the source.
    let (begin, end) = (
        ["-----BEGIN", "CERTIFICATE-----"].join(" "),
        ["-----END", "CERTIFICATE-----"].join(" "),
    );
    let path = directory.join("internal-ca.pem");
    std::fs::write(&path, format!("{begin}\n{}\n{end}\n", lines.join("\n")))?;
    Ok(path)
}

// TEST: GW-I-175
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_175_a_keyed_host_behind_an_internal_ca_is_reached_with_egress_ca_file(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    let directory = tempfile::tempdir()?;
    let ca = pem_file(directory.path(), &hosts.ca)?;
    let file = config(5).replacen(
        "[egress]\n",
        &format!("[egress]\nca_file={}\n", serde_json::to_string(&ca)?),
        1,
    );
    support::run_configured(pool, None, &file, async |f| {
        let run = started(f, &hosts).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        let mut client = keyed_client(run.proxy, HOST, run.install.clone()).await?;
        assert_eq!(
            send(&mut client, get(HOST, "/v1/me", Some(&bearer))?).await?,
            (200, "ok".to_owned()),
            "verified against the configured CA, with no test hook"
        );
        // A certificate from a CA the file doesn't name is still refused.
        let mut bad = keyed_client(run.proxy, "bad.keyed.test", run.install.clone()).await?;
        let (status, words) = send(&mut bad, get("bad.keyed.test", "/", Some(&bearer))?).await?;
        assert!(
            status == 502 && words.contains("couldn't be verified"),
            "{status} {words}"
        );
        Ok(())
    })
    .await
}

/// The first line of the proxy's answer to a CONNECT for `authority`.
async fn connect_answer(proxy: SocketAddr, authority: &str) -> Result<String, Error> {
    let mut socket = tokio::net::TcpStream::connect(proxy).await?;
    socket
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await?;
    let mut answer = vec![0_u8; 256];
    let read = socket.read(&mut answer).await?;
    let text = String::from_utf8_lossy(&answer[..read]).into_owned();
    Ok(text.lines().next().unwrap_or_default().to_owned())
}

/// The keys of GW-I-176: an IPv4 and an IPv6 address (typed in another spelling), and an address with no rule.
const ADDRESS_KEYS: [(&str, &str); 3] = [
    ("V4_KEY", "https://192.0.2.34"),
    ("V6_KEY", "https://[2001:DB8:0::34]"),
    ("UNLISTED_KEY", "https://192.0.2.35"),
];

// TEST: GW-I-176
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_176_a_key_for_an_exact_address_is_used_through_that_addresss_rule(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        f.gateway.trust_upstream_root(&hosts.ca)?;
        let run = started_with(f, &hosts, &ADDRESS_KEYS).await?;
        let bearer = |name: &str| format!("Bearer {}", run.placeholders[name]);
        // IPv4: the leaf names the address; verified upstream by the address.
        let mut v4 = client_at(run.proxy, &format!("{V4}:443"), V4, run.install.clone()).await?;
        assert_eq!(
            send(&mut v4, get(V4, "/v1/me", Some(&bearer("V4_KEY")))?).await?,
            (200, "ok".to_owned())
        );
        // IPv6: another spelling in the CONNECT, the Host in brackets; one address.
        let mut v6 = client_at(run.proxy, "[2001:DB8::0:34]:443", V6, run.install.clone()).await?;
        assert_eq!(
            send(
                &mut v6,
                get("[2001:db8::34]", "/v1/me", Some(&bearer("V6_KEY")))?
            )
            .await?,
            (200, "ok".to_owned())
        );
        for seen in [&hosts.by_address.0, &hosts.by_address.1] {
            let seen = seen.0.lock().map_err(|_| "lock")?.clone();
            assert_eq!(
                seen.iter()
                    .map(|sent| sent.authorization.as_str())
                    .collect::<Vec<_>>(),
                [format!("Bearer {VALUE}")]
            );
        }
        assert_eq!(uses(f, "ok").await?, 2);
        Ok(())
    })
    .await
}

// TEST: GW-I-176
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_176_an_address_without_its_rule_or_another_address_never_gets_the_key(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        f.gateway.trust_upstream_root(&hosts.ca)?;
        let run = started_with(f, &hosts, &ADDRESS_KEYS).await?;
        // A key doesn't make an address reachable: without its own rule, the tunnel is refused.
        let refused = connect_answer(run.proxy, &format!("{UNLISTED}:443")).await?;
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
        // On the IPv4 address's tunnel, a request naming another address is refused, and nothing reaches the host.
        let mut v4 = client_at(run.proxy, &format!("{V4}:443"), V4, run.install.clone()).await?;
        let placeholder = format!("Bearer {}", run.placeholders["V4_KEY"]);
        let (status, _) = send(&mut v4, get(UNLISTED, "/", Some(&placeholder))?).await?;
        assert_ne!(status, 200);
        assert!(hosts.by_address.0.0.lock().map_err(|_| "lock")?.is_empty());
        Ok(())
    })
    .await
}

// TEST: GW-I-166
/// A request inside a keyed tunnel names its host only in `Host`: a target with a scheme and authority, which a server
/// or a shared front may route by over `Host` (RFC 9112 §3.2.2), is refused before anything is judged, recorded or
/// sent, whatever host it names (ADR-0037 §4: domain fronting).
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_166_an_absolute_target_is_refused_and_never_reaches_a_host(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        for (target, authorization) in [
            ("https://other.keyed.test/v1/me", Some(bearer.as_str())),
            ("https://api.keyed.test/v1/me", Some(bearer.as_str())),
            ("https://other.keyed.test/v1/me", None),
        ] {
            let mut client = keyed_client(run.proxy, HOST, run.install.clone()).await?;
            let (status, words) = send(&mut client, get(HOST, target, authorization)?).await?;
            assert_eq!(status, 403, "{target}: {words}");
            assert!(words.contains("by its path"), "{target}: {words}");
        }
        assert!(
            hosts.seen.0.lock().map_err(|_| "lock")?.is_empty(),
            "the host received nothing"
        );
        assert_eq!(uses(f, "ok").await?, 0, "nothing was swapped");
        Ok(())
    })
    .await
}

// TEST: GW-I-166
/// A key's value never comes back on any keyed connection of a run that holds the key: not on another connection
/// than the one that swapped it, not on a connection's first request, before any swap of its own, and not to another
/// task of the same person (ADR-0037 §6): what a person's runs swapped is scrubbed on every keyed
/// connection of theirs to that host. The host here sends back the credential it saw last, as a session or a "show my
/// token" page does.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_166_a_value_sent_back_on_another_connection_is_scrubbed(pool: PgPool) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        let mut first = keyed_client(run.proxy, HOST, run.install.clone()).await?;
        assert_eq!(
            send(&mut first, get(HOST, "/v1/me", Some(&bearer))?).await?,
            (200, "ok".to_owned())
        );
        // A second connection, its first request carrying no placeholder.
        let mut second = keyed_client(run.proxy, HOST, run.install.clone()).await?;
        let (header, body) = echo(&mut second, get(HOST, "/last", None)?).await?;
        assert!(
            !header.contains(VALUE) && !body.contains(VALUE),
            "the value came back: {header} {body}"
        );
        assert_eq!(
            (header.as_str(), body),
            (bearer.as_str(), format!("last {bearer}"))
        );
        // Another task of the same person, its own sandbox and run, before any swap of its own.
        let other = task_run(f, (run.owner.0, &run.owner.1), run.proxy, "API_KEY").await?;
        assert_ne!(other.placeholder, run.placeholder);
        // Two runtimes share loopback here, so this one names its sandbox to the proxy.
        let authority = format!("{HOST}:443");
        let mut third = client_as(
            other.proxy,
            (&authority, HOST),
            other.install.clone(),
            Some(&other.token),
        )
        .await?;
        let (header, body) = echo(&mut third, get(HOST, "/last", None)?).await?;
        assert!(
            !header.contains(VALUE) && !body.contains(VALUE),
            "the value came back: {header} {body}"
        );
        assert_eq!(
            (header.as_str(), body),
            (bearer.as_str(), format!("last {bearer}"))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-166
/// A placeholder in a request body is refused like one anywhere else but the Authorization header: in words, counted
/// and told as `wrong_position`, and never sent (ADR-0037 §4). A body streams, so the head and the body
/// before it may have gone; when the same request swapped its header's placeholder, that swap stands, counted `ok`.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_166_a_placeholder_in_a_body_is_a_counted_refusal_in_words(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        let upload = |authorization: Option<&str>| -> Result<Request<Full<Bytes>>, Error> {
            let mut request = Request::post("/upload").header("host", HOST);
            if let Some(authorization) = authorization {
                request = request.header("authorization", authorization);
            }
            Ok(request.body(Full::new(Bytes::from(format!(
                "data {} more",
                run.placeholder
            ))))?)
        };
        for (authorization, swapped) in [(None, 0), (Some(bearer.as_str()), 1)] {
            let mut client = keyed_client(run.proxy, HOST, run.install.clone()).await?;
            let (status, words) = send(&mut client, upload(authorization)?).await?;
            assert!(
                status == 403 && words.contains("Authorization header"),
                "{status} {words}"
            );
            assert_eq!(uses(f, "ok").await?, swapped);
        }
        assert_eq!(
            uses(f, "wrong_position").await?,
            2,
            "each body's placeholder is counted"
        );
        let seen = hosts.seen.0.lock().map_err(|_| "lock")?.clone();
        assert!(
            seen.iter().all(|sent| !sent.body.contains("tqa_ph_")),
            "{seen:?}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-166
/// A running run's placeholder is judged against its key as it is now, not as it was when the run began: a key its
/// owner moved to another address is refused at the old one, `wrong_host` naming the new; a shared key whose person was
/// taken off its users is `revoked`. Nothing is sent either way.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_166_a_key_changed_mid_run_is_judged_as_it_is_now(pool: PgPool) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let cookie = run.owner.1.clone();
        let bearer = format!("Bearer {}", run.placeholder);
        let (_, listed) = call(f, Method::GET, "/api/v1/host-keys", &cookie, (None, None)).await?;
        let key = listed["items"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["name"] == "API_KEY"))
            .ok_or("API_KEY")?
            .clone();
        let moved = serde_json::json!({"version": key["version"], "name": "API_KEY",
            "address": "https://moved.keyed.test", "scheme": "bearer"});
        let path = format!("/api/v1/host-keys/{}", key["id"].as_str().ok_or("id")?);
        let (status, answer) = call(
            f,
            Method::PUT,
            &path,
            &cookie,
            (Some(moved), Some("move-1")),
        )
        .await?;
        assert_eq!(status, 200, "{answer}");
        let mut client = keyed_client(run.proxy, HOST, run.install.clone()).await?;
        let (status, words) = send(&mut client, get(HOST, "/v1/me", Some(&bearer))?).await?;
        assert!(
            status == 403 && words.contains("moved.keyed.test"),
            "{status} {words}"
        );
        // A shared key the owner may use, then is taken off of, while a second task's run holds it.
        let shared = serde_json::json!({"name": "SHARED_KEY", "address": format!("https://{HOST}"),
            "scheme": "bearer", "value": VALUE, "users": [run.owner.0]});
        let (status, added) = call(
            f,
            Method::POST,
            "/api/v1/admin/host-keys",
            &cookie,
            (Some(shared), Some("shared-1")),
        )
        .await?;
        assert_eq!(status, 201, "{added}");
        let other = task_run(f, (run.owner.0, &cookie), run.proxy, "SHARED_KEY").await?;
        let removed = serde_json::json!({"version": added["version"], "name": "SHARED_KEY",
            "address": format!("https://{HOST}"), "scheme": "bearer", "users": []});
        let path = format!(
            "/api/v1/admin/host-keys/{}",
            added["id"].as_str().ok_or("id")?
        );
        let (status, answer) = call(
            f,
            Method::PUT,
            &path,
            &cookie,
            (Some(removed), Some("shared-2")),
        )
        .await?;
        assert_eq!(status, 200, "{answer}");
        let authority = format!("{HOST}:443");
        let mut client = client_as(
            other.proxy,
            (&authority, HOST),
            other.install.clone(),
            Some(&other.token),
        )
        .await?;
        let shared_bearer = format!("Bearer {}", other.placeholder);
        let (status, words) = send(&mut client, get(HOST, "/v1/me", Some(&shared_bearer))?).await?;
        assert!(
            status == 403 && words.contains("can no longer use it"),
            "{status} {words}"
        );
        // The refused use is still a counted row (ADR-0037's R4), whoever may no longer use the key.
        let reads = format!(
            "/api/v1/admin/host-keys/{}/reads?limit=10",
            added["id"].as_str().ok_or("id")?
        );
        let (_, rows) = call(f, Method::GET, &reads, &cookie, (None, None)).await?;
        assert!(
            rows["items"]
                .as_array()
                .is_some_and(|items| items.iter().any(|row| row["outcome"] == "revoked")),
            "{rows}"
        );
        assert!(
            hosts.seen.0.lock().map_err(|_| "lock")?.is_empty(),
            "nothing was sent"
        );
        assert_eq!(uses(f, "ok").await?, 0);
        Ok(())
    })
    .await
}
