//! Internal rules through the real proxy (ADR-0035 amendment, the owner's decision of 1 Oct 2026):
//! - SEC-050: an internal rule never reaches the engine's networks, the sandbox pool, the gateway's own addresses,
//!   `internal_never`, loopback or cloud metadata.
//! - SEC-051: DNS rebinding: each connection is judged on its own answer, which must be wholly allowed.
//! - SEC-052: without the switch, a name resolving to a private address is refused, as before.
//! - SEC-053: the switch is off by default, and only for one exact host name (API, file and database).
//! - SEC-054: internal rules refuse in words until the host's addresses are known (the engine-unknown and stale cases are
//!   the unit tests', which drive the clock).
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::{Fixture, TestResult, call};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Error = Box<dyn std::error::Error>;

/// Names to the addresses a test sets (re-pointable), and every address to one local destination.
struct Resolver {
    names: Mutex<HashMap<String, Vec<IpAddr>>>,
    destination: SocketAddr,
}

#[async_trait::async_trait]
impl tqa_gateway::EgressResolver for Resolver {
    async fn resolve(&self, host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
        let names = self
            .names
            .lock()
            .map_err(|_| std::io::Error::other("lock"))?;
        let addresses = names
            .get(host)
            .ok_or_else(|| std::io::Error::other("unknown name"))?;
        Ok(addresses
            .iter()
            .map(|address| SocketAddr::new(*address, port))
            .collect())
    }

    async fn connect(&self, _: SocketAddr) -> std::io::Result<tokio::net::TcpStream> {
        tokio::net::TcpStream::connect(self.destination).await
    }
}

impl Resolver {
    fn point(&self, host: &str, addresses: &[&str]) -> TestResult {
        let addresses = addresses
            .iter()
            .map(|text| text.parse())
            .collect::<Result<_, _>>()?;
        self.names
            .lock()
            .map_err(|_| "lock")?
            .insert(host.to_owned(), addresses);
        Ok(())
    }
}

/// A destination that counts the connections it accepts.
async fn destination() -> Result<(SocketAddr, Arc<AtomicUsize>), Error> {
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

/// The proxy's answer to a CONNECT for `authority`: its status line and the rest it sent.
async fn connect(proxy: SocketAddr, authority: &str) -> Result<(String, String), Error> {
    let mut socket = tokio::net::TcpStream::connect(proxy).await?;
    socket
        .write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await?;
    let mut answer = vec![0_u8; 4096];
    let read = tokio::time::timeout(support::DEADLINE, socket.read(&mut answer)).await??;
    let text = String::from_utf8_lossy(&answer[..read]).into_owned();
    let (status, rest) = text.split_once("\r\n").unwrap_or((&text, ""));
    Ok((status.to_owned(), rest.to_owned()))
}

const INTERNAL: &str = "git.corp.test";
const PLAIN: &str = "plain.corp.test";

/// The gateway's file: an internal rule and a plain one for names on the internal network, and `never` as
/// `internal_never` (none when empty).
fn config(never: &str) -> String {
    let never = if never.is_empty() {
        String::new()
    } else {
        format!("internal_never=['{never}']\n")
    };
    format!(
        "[egress]\nlisten='127.0.0.1:3128'\n{never}[[egress.allow]]\nhost='{INTERNAL}'\nports=[443]\ninternal=true\n\
         [[egress.allow]]\nhost='{PLAIN}'\nports=[443]\n"
    )
}

/// The proxy with the engine's default bridge as its only network and the gateway at 192.168.4.16 inside a container
/// (it holds no bridge gateway, so it doesn't see the host's interfaces).
/// The caller is a task's sandbox: its runtime is connected, so the proxy identifies it (kept for the test's length).
struct Proxied {
    proxy: SocketAddr,
    resolver: Arc<Resolver>,
    accepted: Arc<AtomicUsize>,
    _runtime: support::runtime::Runtime,
}

async fn proxy(f: &Fixture, owner: uuid::Uuid) -> Result<Proxied, Error> {
    let (destination, accepted) = destination().await?;
    *f.backend.engine.lock().map_err(|_| "lock")? = Some(tqa_sandbox::EngineNetworks {
        subnets: vec!["192.168.4.0/27".into()],
        gateways: vec!["192.168.4.1".parse()?],
    });
    f.gateway
        .internal_inventory_now(vec!["192.168.4.16".parse()?])
        .await;
    let resolver = Arc::new(Resolver {
        names: Mutex::default(),
        destination,
    });
    let (proxy, _task) = f.egress(resolver.clone()).await?;
    let task = f.conversation(owner).await?;
    let runtime = f.runtime(&task).await?;
    Ok(Proxied {
        proxy,
        resolver,
        accepted,
        _runtime: runtime,
    })
}

// TEST: SEC-051
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_051_each_connection_is_judged_on_its_own_whole_answer(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &config("192.168.1.1"), async |f| {
        let (admin, _) = f.admin().await?;
        let Proxied {
            proxy,
            resolver,
            accepted,
            _runtime,
        } = proxy(f, admin.id).await?;
        let target = format!("{INTERNAL}:443");
        resolver.point(INTERNAL, &["10.1.2.3"])?;
        assert_eq!(
            connect(proxy, &target).await?.0,
            "HTTP/1.1 200 Connection Established"
        );
        // Re-pointed between connections: judged again, never on the earlier answer.
        resolver.point(INTERNAL, &["127.0.0.1"])?;
        assert!(connect(proxy, &target).await?.0.starts_with("HTTP/1.1 403"));
        // One forbidden address in an answer refuses the whole answer, never a filtered part of it.
        resolver.point(INTERNAL, &["10.1.2.3", "192.168.4.1"])?;
        assert!(connect(proxy, &target).await?.0.starts_with("HTTP/1.1 403"));
        resolver.point(INTERNAL, &["10.1.2.3", "10.0.0.5"])?;
        assert_eq!(
            connect(proxy, &target).await?.0,
            "HTTP/1.1 200 Connection Established"
        );
        tokio::time::timeout(support::DEADLINE, async {
            while accepted.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        assert_eq!(
            accepted.load(Ordering::SeqCst),
            2,
            "only the allowed answers reached anything"
        );
        Ok(())
    })
    .await
}

// TEST: SEC-050
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_050_an_internal_rule_never_reaches_what_the_gateway_stands_on(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &config("192.168.1.1"), async |f| {
        let (admin, _) = f.admin().await?;
        let Proxied {
            proxy,
            resolver,
            accepted,
            _runtime,
        } = proxy(f, admin.id).await?;
        let target = format!("{INTERNAL}:443");
        for (address, words) in [
            ("192.168.4.0", "never use here"),  // the engine's bridge
            ("192.168.4.1", "never use here"),  // the host on that bridge
            ("192.168.4.16", "never use here"), // the gateway itself
            ("10.213.0.0", "never use here"),   // the sandbox pool
            ("192.168.1.1", "never use here"),  // the host's LAN address, from internal_never
            ("127.0.0.1", "private or reserved"),
            ("169.254.169.254", "private or reserved"),
        ] {
            resolver.point(INTERNAL, &[address])?;
            let (status, rest) = connect(proxy, &target).await?;
            assert!(status.starts_with("HTTP/1.1 403"), "{address}: {status}");
            assert!(rest.contains(words), "{address}: {rest}");
        }
        assert_eq!(accepted.load(Ordering::SeqCst), 0);
        Ok(())
    })
    .await
}

// TEST: SEC-052
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_052_without_the_switch_a_private_answer_is_refused(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &config("192.168.1.1"), async |f| {
        let (admin, _) = f.admin().await?;
        let Proxied {
            proxy,
            resolver,
            accepted,
            _runtime,
        } = proxy(f, admin.id).await?;
        resolver.point(PLAIN, &["10.1.2.3"])?;
        let (status, rest) = connect(proxy, &format!("{PLAIN}:443")).await?;
        assert!(status.starts_with("HTTP/1.1 403"), "{status}");
        assert!(rest.contains("private or reserved"), "{rest}");
        assert_eq!(accepted.load(Ordering::SeqCst), 0);
        Ok(())
    })
    .await
}

/// The admin list's item for `host`.
async fn listed(f: &Fixture, cookie: &str, host: &str) -> Result<Value, Error> {
    let (status, body) = call(f, Method::GET, "/api/v1/admin/egress", cookie, (None, None)).await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["host"] == host)
        .cloned()
        .ok_or("the rule")?)
}

// TEST: SEC-054
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_054_internal_rules_refuse_in_words_until_the_host_is_named(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &config(""), async |f| {
        let (admin, cookie) = f.admin().await?;
        let Proxied {
            proxy,
            resolver,
            accepted,
            _runtime,
        } = proxy(f, admin.id).await?;
        resolver.point(INTERNAL, &["10.1.2.3"])?;
        let (status, rest) = connect(proxy, &format!("{INTERNAL}:443")).await?;
        assert!(status.starts_with("HTTP/1.1 403"), "{status}");
        assert!(rest.contains("internal servers are off"), "{rest}");
        assert_eq!(
            listed(f, &cookie, INTERNAL).await?["internal_state"],
            "internal_never_missing"
        );
        assert_eq!(listed(f, &cookie, PLAIN).await?.get("internal_state"), None);
        // In the host's network namespace (it holds the bridge's gateway), it needs no internal_never.
        f.gateway
            .internal_inventory_now(vec!["192.168.4.1".parse()?, "192.168.1.1".parse()?])
            .await;
        assert_eq!(
            connect(proxy, &format!("{INTERNAL}:443")).await?.0,
            "HTTP/1.1 200 Connection Established"
        );
        assert_eq!(
            listed(f, &cookie, INTERNAL).await?.get("internal_state"),
            None
        );
        // An engine that can't be asked keeps the last inventory until it goes stale (three minutes; SEC-054's unit
        // tests drive that clock), so a renewal failure alone changes nothing at once.
        *f.backend.engine.lock().map_err(|_| "lock")? = None;
        f.gateway
            .internal_inventory_now(vec!["192.168.4.1".parse()?])
            .await;
        assert_eq!(
            connect(proxy, &format!("{INTERNAL}:443")).await?.0,
            "HTTP/1.1 200 Connection Established"
        );
        assert_eq!(accepted.load(Ordering::SeqCst), 2);
        Ok(())
    })
    .await
}

async fn put(f: &Fixture, cookie: &str, host: &str, body: Value) -> Result<(u16, Value), Error> {
    call(
        f,
        Method::PUT,
        &format!("/api/v1/admin/egress/{host}"),
        cookie,
        (Some(body), None),
    )
    .await
}

// TEST: SEC-053
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_053_the_switch_is_off_by_default_and_only_for_one_name(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (status, body) = put(f, &cookie, "files.corp.test", json!({"ports": [443]})).await?;
        assert_eq!(status, 201, "{body}");
        assert_eq!(body["internal"], false, "off unless set");
        for host in ["*.corp.test", "10.0.0.5"] {
            let (status, body) =
                put(f, &cookie, host, json!({"ports": [443], "internal": true})).await?;
            assert_eq!(status, 400, "{host}: {body}");
            assert_eq!(
                body["error"]["details"][0]["reason"], "internal_name_only",
                "{host}"
            );
        }
        let (status, body) = put(
            f,
            &cookie,
            "git.corp.test",
            json!({"ports": [443], "internal": true}),
        )
        .await?;
        assert_eq!((status, &body["internal"]), (201, &json!(true)), "{body}");
        // The database's own default and rule, whatever writes to it.
        sqlx::query("INSERT INTO egress_rules (host, ports) VALUES ('raw.corp.test', '{443}')")
            .execute(&f.pool)
            .await?;
        let internal: bool =
            sqlx::query_scalar("SELECT internal FROM egress_rules WHERE host='raw.corp.test'")
                .fetch_one(&f.pool)
                .await?;
        assert!(!internal);
        let refused = sqlx::query(
            "INSERT INTO egress_rules (host, ports, internal) VALUES ('*.raw.test', '{443}', true)",
        )
        .execute(&f.pool)
        .await;
        assert!(
            refused.is_err(),
            "the database refuses internal on a wildcard"
        );
        Ok(())
    })
    .await
}
