//! SEC-022 (ADR-0028): the policy proxy always runs and every sandbox is attached to it from creation, so the live
//! allow-list alone decides; with no rules it denies everything. Only `[egress] disabled = true` leaves sandboxes
//! internal-only. A rule added or removed through the admin API applies to an existing sandbox's next new connection,
//! and a tunnel already open runs on.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use sqlx::PgPool;
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::{Fixture, TestResult};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A destination that counts every connection it accepts.
async fn destination() -> Result<(SocketAddr, Arc<AtomicUsize>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&accepted);
    tokio::spawn(async move {
        while let Ok((_socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
        }
    });
    Ok((address, accepted))
}

/// A destination that counts connections and echoes what each one sends.
async fn echo() -> Result<(SocketAddr, Arc<AtomicUsize>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&accepted);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let (mut reader, mut writer) = socket.split();
                let _ = tokio::io::copy(&mut reader, &mut writer).await;
            });
        }
    });
    Ok((address, accepted))
}

/// Sends `text` through an open tunnel and returns what came back.
async fn round_trip(
    stream: &mut tokio::net::TcpStream,
    text: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    stream.write_all(text.as_bytes()).await?;
    let mut reply = vec![0; text.len()];
    tokio::time::timeout(support::DEADLINE, stream.read_exact(&mut reply)).await??;
    Ok(String::from_utf8(reply)?)
}

/// Sends a CONNECT for `target` through the proxy and returns the proxy's status line.
async fn connect(
    proxy: SocketAddr,
    target: SocketAddr,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(tunnel(proxy, target).await?.0)
}

/// Sends a CONNECT for `target` and returns the proxy's status line with the stream, open for a tunnel.
async fn tunnel(
    proxy: SocketAddr,
    target: SocketAddr,
) -> Result<(String, tokio::net::TcpStream), Box<dyn std::error::Error>> {
    let mut stream =
        tokio::time::timeout(support::DEADLINE, tokio::net::TcpStream::connect(proxy)).await??;
    stream
        .write_all(format!("CONNECT {target} HTTP/1.1\r\n\r\n").as_bytes())
        .await?;
    let mut reply = vec![0; 256];
    let read = tokio::time::timeout(support::DEADLINE, stream.read(&mut reply)).await??;
    let status = String::from_utf8_lossy(&reply[..read])
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    Ok((status, stream))
}

/// Binds a runtime for a new conversation, so the proxy knows the caller, and returns the sandbox's proxy URL.
async fn sandbox(f: &Fixture) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let (user, _) = f.admin().await?;
    let conversation = f.conversation(user.id).await?;
    let _runtime = f.runtime(&conversation).await?;
    Ok(f.backend.proxy(&conversation.id))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_022_without_rules_every_sandbox_is_attached_and_everything_is_denied(
    pool: PgPool,
) -> TestResult {
    let (target, accepted) = destination().await?;
    support::run_configured(pool, None, "", async |f| {
        let (proxy, _task) = f.egress(Arc::new(tqa_gateway::SystemResolver)).await?;
        let attached = sandbox(f).await?;
        assert_eq!(
            attached.as_deref(),
            Some("http://gateway:3128/"),
            "attached from creation"
        );
        let status = connect(proxy, target).await?;
        assert!(status.starts_with("HTTP/1.1 403"), "{status}");
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            0,
            "nothing reached the destination"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_022_only_the_explicit_switch_leaves_sandboxes_internal(pool: PgPool) -> TestResult {
    let (target, accepted) = destination().await?;
    support::run_configured(pool, None, "[egress]\ndisabled=true", async |f| {
        let (proxy, _task) = f.egress(Arc::new(tqa_gateway::SystemResolver)).await?;
        assert_eq!(
            sandbox(f).await?,
            None,
            "an internal-only sandbox has no proxy"
        );
        let status = connect(proxy, target).await?;
        assert!(status.starts_with("HTTP/1.1 403"), "{status}");
        assert_eq!(accepted.load(Ordering::SeqCst), 0);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_022_a_rule_change_applies_to_a_running_sandbox_from_its_next_connection(
    pool: PgPool,
) -> TestResult {
    let (target, accepted) = echo().await?;
    support::run_configured(pool, None, "", async |f| {
        let (proxy, _task) = f.egress(Arc::new(tqa_gateway::SystemResolver)).await?;
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let _runtime = f.runtime(&conversation).await?;
        let attached = f.backend.proxy(&conversation.id);
        // GW-I-012: the refusal names the host and port (ADR-0035 §3).
        assert!(connect(proxy, target).await?.starts_with(&format!(
            "HTTP/1.1 403 Blocked by the network allow-list: {target}"
        )));
        let rule = format!("/api/v1/admin/egress/{}", target.ip());
        let body = Some(serde_json::json!({"ports": [target.port()]}));
        assert_eq!(
            support::call(f, Method::PUT, &rule, &cookie, (body, None))
                .await?
                .0,
            201
        );
        let (status, mut open) = tunnel(proxy, target).await?;
        assert!(
            status.starts_with("HTTP/1.1 200"),
            "the first rule reaches the running sandbox: {status}"
        );
        assert_eq!(round_trip(&mut open, "before").await?, "before");
        let removal = format!("{rule}?version=1");
        assert_eq!(
            support::call(f, Method::DELETE, &removal, &cookie, (None, None))
                .await?
                .0,
            204
        );
        let status = connect(proxy, target).await?;
        assert!(
            status.starts_with("HTTP/1.1 403"),
            "the next connection is denied: {status}"
        );
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            1,
            "only the allowed tunnel reached the destination"
        );
        assert_eq!(
            round_trip(&mut open, "after").await?,
            "after",
            "an open tunnel runs to its own bounds"
        );
        assert_eq!(
            f.backend.proxy(&conversation.id),
            attached,
            "the attachment is unchanged"
        );
        Ok(())
    })
    .await
}
