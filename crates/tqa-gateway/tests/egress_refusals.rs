//! GW-I-323 (ADR-0035 point 3): the recent refusals an administrator reads under Settings › Internet access, and
//! "Allow this site". The proxy keeps a bounded, in-memory record of the destinations it refused: host, port, reason,
//! how often and when last, and which list refused them. A refusal the allow-list made can be allowed with the existing
//! rule write (audited as any rule change); one the address guard made can't, since a rule wouldn't open it.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{net::SocketAddr, sync::Arc};
use support::{Fixture, TestResult};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Sends a CONNECT for `authority` through the proxy and returns the proxy's status line.
async fn connect(proxy: SocketAddr, authority: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut stream =
        tokio::time::timeout(support::DEADLINE, tokio::net::TcpStream::connect(proxy)).await??;
    stream
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\n\r\n").as_bytes())
        .await?;
    let mut reply = vec![0; 256];
    let read = tokio::time::timeout(support::DEADLINE, stream.read(&mut reply)).await??;
    Ok(String::from_utf8_lossy(&reply[..read])
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned())
}

async fn refusals(f: &Fixture, cookie: &str) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    let response = f.get("/api/v1/admin/egress/refusals", cookie).await?;
    let status = response.status().as_u16();
    Ok((
        status,
        response.json::<Value>().await.unwrap_or(Value::Null),
    ))
}

fn entry<'a>(listed: &'a Value, host: &str, port: u16) -> Option<&'a Value> {
    listed["items"]
        .as_array()?
        .iter()
        .find(|item| item["host"] == host && item["port"] == port)
}

// TEST: GW-I-323
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_323_a_refused_site_is_listed_and_allowing_it_opens_the_next_connection(
    pool: PgPool,
) -> TestResult {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let target = listener.local_addr()?;
    tokio::spawn(async move { while listener.accept().await.is_ok() {} });
    support::run_configured(pool, None, "", async |f| {
        let (proxy, _task) = f.egress(Arc::new(tqa_gateway::SystemResolver)).await?;
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let _runtime = f.runtime(&conversation).await?;
        let (_, member) = f.member("member@example.invalid").await?;

        // Nothing refused yet.
        let (status, listed) = refusals(f, &cookie).await?;
        assert_eq!(status, 200);
        assert_eq!(listed["items"], json!([]));

        // The same site refused twice by the allow-list, and a name the address guard refuses.
        let host = target.ip().to_string();
        for _ in 0..2 {
            assert!(
                connect(proxy, &target.to_string())
                    .await?
                    .starts_with("HTTP/1.1 403")
            );
        }
        let (_, listed) = refusals(f, &cookie).await?;
        let refused = entry(&listed, &host, target.port()).ok_or("the refused site is listed")?;
        assert_eq!(refused["reason"], "not_listed");
        assert_eq!(refused["count"], 2);
        assert_eq!(refused["list"], "install");
        assert_eq!(refused["allowable"], true);
        assert_eq!(refused["allowed"], false);
        assert!(
            refused["last_at"]
                .as_str()
                .is_some_and(|at| at.ends_with('Z')),
            "{refused}"
        );

        // Only an administrator reads them.
        assert_eq!(refusals(f, &member).await?.0, 403);

        // Allow this site: the existing rule write, with its audit; the next connection is decided by the new list.
        let rule = format!("/api/v1/admin/egress/{host}");
        let body = Some(json!({"ports": [target.port()]}));
        assert_eq!(
            support::call(f, Method::PUT, &rule, &cookie, (body, None))
                .await?
                .0,
            201
        );
        assert!(
            connect(proxy, &target.to_string())
                .await?
                .starts_with("HTTP/1.1 200")
        );
        let (_, listed) = refusals(f, &cookie).await?;
        let allowed = entry(&listed, &host, target.port()).ok_or("still listed")?;
        assert_eq!(allowed["allowed"], true, "{allowed}");
        assert_eq!(allowed["count"], 2, "an allowed connection is no refusal");
        Ok(())
    })
    .await
}

// TEST: GW-I-323
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_323_a_refusal_no_rule_would_open_is_listed_without_allow(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "", async |f| {
        let (proxy, _task) = f.egress(Arc::new(tqa_gateway::SystemResolver)).await?;
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let _runtime = f.runtime(&conversation).await?;
        // A name on the list that resolves to a private address: the guard refuses it, and a rule can't change that.
        let rule = "/api/v1/admin/egress/localhost";
        let body = Some(json!({"ports": [9]}));
        assert_eq!(
            support::call(f, Method::PUT, rule, &cookie, (body, None))
                .await?
                .0,
            201
        );
        assert!(
            connect(proxy, "localhost:9")
                .await?
                .starts_with("HTTP/1.1 403")
        );
        let (_, listed) = refusals(f, &cookie).await?;
        let refused = entry(&listed, "localhost", 9).ok_or("listed")?;
        assert_eq!(refused["reason"], "private");
        assert_eq!(refused["allowable"], false);
        Ok(())
    })
    .await
}
