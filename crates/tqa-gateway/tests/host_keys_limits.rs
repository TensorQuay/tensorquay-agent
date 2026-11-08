//! GW-I-193 and GW-I-194 (ADR-0037 §4, point 4): the first slice's limits on a keyed host, held so the next slice knows
//! what it lifts. A client offering HTTP/2 is answered over HTTP/1.1, and an upgrade (WebSocket) never opens: the
//! proxy drops `Upgrade` and `Connection`, so the host answers the request as a plain one.
#[path = "support/keyed.rs"]
mod keyed;
#[path = "support/fixture.rs"]
mod support;

use keyed::*;
use sqlx::PgPool;
use std::sync::Arc;
use support::TestResult;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// TEST: GW-I-193
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_193_a_client_offering_http2_on_a_keyed_host_gets_http1(pool: PgPool) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let mut socket = tokio::net::TcpStream::connect(run.proxy).await?;
        let authority = format!("{HOST}:443");
        socket
            .write_all(
                format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes(),
            )
            .await?;
        let mut answer = [0_u8; 39];
        socket.read_exact(&mut answer).await?;
        assert_eq!(&answer, b"HTTP/1.1 200 Connection Established\r\n\r\n");
        let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(run.install.clone())
        .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(rustls::pki_types::ServerName::try_from(HOST)?, socket)
            .await?;
        assert_eq!(
            tls.get_ref().1.alpn_protocol(),
            Some(&b"http/1.1"[..]),
            "HTTP/2 is a later step"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-194
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_194_an_upgrade_on_a_keyed_host_never_opens(pool: PgPool) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        let run = keyed_run(f, &hosts).await?;
        let mut client = keyed_client(run.proxy, HOST, run.install.clone()).await?;
        let mut request = get(
            HOST,
            "/upgrade",
            Some(&format!("Bearer {}", run.placeholder)),
        )?;
        let headers = request.headers_mut();
        headers.insert(
            "connection",
            hyper::header::HeaderValue::from_static("Upgrade"),
        );
        headers.insert(
            "upgrade",
            hyper::header::HeaderValue::from_static("websocket"),
        );
        headers.insert(
            "sec-websocket-version",
            hyper::header::HeaderValue::from_static("13"),
        );
        headers.insert(
            "sec-websocket-key",
            hyper::header::HeaderValue::from_static("dGhlIHNhbXBsZSBub25jZQ=="),
        );
        let (status, _) = send(&mut client, request).await?;
        assert_eq!(
            status, 426,
            "the host saw no upgrade, so none opened; upgrades are a later step"
        );
        let seen = hosts.seen.0.lock().map_err(|_| "lock")?.clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            seen[0].authorization,
            format!("Bearer {VALUE}"),
            "the request itself went, swapped"
        );
        Ok(())
    })
    .await
}
