//! SBX-U-005: a sandbox network is created with a pool subnet and the gateway attached, and every failure unwinds in
//! reverse without leaking a subnet (ADR-0021 amendment). A scripted Docker answers each request in order.
use std::{collections::VecDeque, time::Duration};

use bollard::{API_DEFAULT_VERSION, Docker};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

use super::{DockerBackend, DockerBackendConfig, SubnetPool, pool::Cidr};
use crate::SandboxError;

pub(super) type TestResult = Result<(), Box<dyn std::error::Error>>;

/// One scripted answer: the request line must contain `expect`; the reply is `status` with `body`.
pub(super) struct Step {
    expect: &'static str,
    status: &'static str,
    body: String,
    /// How long the daemon takes before it answers (a loaded daemon).
    delay: Duration,
}

pub(super) fn step(expect: &'static str, status: &'static str, body: &str) -> Step {
    Step {
        expect,
        status,
        body: body.into(),
        delay: Duration::ZERO,
    }
}

/// A step the daemon answers only after `delay`.
pub(super) fn slow(step: Step, delay: Duration) -> Step {
    Step { delay, ..step }
}

/// The inspected network as Docker reports it: this instance's label, its subnet, and attached containers.
pub(super) fn inspected(owner: &str, subnet: &str, gateway_attached: bool) -> String {
    let containers = if gateway_attached {
        r#"{"abc":{"Name":"tqa-gateway"}}"#
    } else {
        "{}"
    };
    format!(
        r#"{{"Name":"tqa-sbx-1","Labels":{{"tqa.instance":"{owner}","tqa.subnet":"{subnet}"}},"Containers":{containers}}}"#
    )
}

/// Serves `steps` in order and returns the request lines it saw; a request that doesn't match its step fails.
async fn serve(
    listener: TcpListener,
    steps: Vec<Step>,
    mut stop: oneshot::Receiver<()>,
) -> Result<Vec<String>, String> {
    let mut steps = VecDeque::from(steps);
    let mut seen = Vec::new();
    loop {
        let (mut stream, _) = tokio::select! {
            _ = &mut stop => return if steps.is_empty() { Ok(seen) } else { Err(format!("unused steps after {seen:?}")) },
            accepted = listener.accept() => accepted.map_err(|error| error.to_string())?,
        };
        let request = read_request(&mut stream).await?;
        let line = String::from_utf8_lossy(&request)
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned();
        let Some(next) = steps.pop_front() else {
            return Err(format!("unexpected request {line}"));
        };
        if !line.contains(next.expect) {
            return Err(format!("expected {} but got {line}", next.expect));
        }
        seen.push(line);
        tokio::time::sleep(next.delay).await;
        let reply = format!(
            "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            next.status,
            next.body.len(),
            next.body
        );
        // A client that stopped waiting (its own timeout) has closed the connection; the reply is simply lost.
        let _ = stream.write_all(reply.as_bytes()).await;
    }
}

/// Whether `request` holds the headers and a body of the announced length.
fn complete(request: &[u8]) -> bool {
    let text = String::from_utf8_lossy(request);
    let Some(end) = text.find("\r\n\r\n") else {
        return false;
    };
    let length = text[..end]
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    request.len() >= end + 4 + length
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<Vec<u8>, String> {
    let mut request = Vec::new();
    let mut buffer = [0; 8192];
    while !complete(&request) {
        let count = stream
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
    }
    Ok(request)
}

/// A backend over a scripted Docker with a two-subnet pool and the gateway container named `tqa-gateway`.
pub(super) async fn scripted_backend(
    gateway: Option<&str>,
) -> Result<(DockerBackend, TcpListener, tempfile::TempDir), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let storage = tempfile::tempdir()?;
    let mut config = DockerBackendConfig::new("tqa-unit", storage.path())?;
    config.set_sandbox_network(
        SubnetPool::new("192.168.4.0/27", 28)?,
        gateway.map(str::to_owned),
    )?;
    Ok((
        DockerBackend::unconnected(
            Docker::connect_with_http(&origin, 5, API_DEFAULT_VERSION)?,
            config,
        ),
        listener,
        storage,
    ))
}

/// Runs `operation` against the scripted Docker and returns its result with the requests seen.
pub(super) async fn scripted<T>(
    listener: TcpListener,
    steps: Vec<Step>,
    operation: std::pin::Pin<Box<dyn std::future::Future<Output = T> + '_>>,
) -> Result<(T, Vec<String>), Box<dyn std::error::Error>> {
    let (stop, stopped) = oneshot::channel();
    let (seen, result) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(serve(listener, steps, stopped), async {
            let result = operation.await;
            let _ = stop.send(());
            result
        })
    })
    .await?;
    Ok((result, seen?))
}

pub(super) fn free(backend: &DockerBackend) -> Result<u32, Box<dyn std::error::Error>> {
    Ok(backend.allocator.lock().map_err(|_| "allocator")?.free())
}

#[tokio::test]
async fn sbx_u_005_a_network_gets_the_lowest_subnet_and_the_gateway_as_gateway() -> TestResult {
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    let (subnet, seen) = scripted(
        listener,
        vec![
            step(
                "POST /networks/create",
                "201 Created",
                r#"{"Id":"n1","Warning":""}"#,
            ),
            step("/networks/tqa-sbx-1/connect", "200 OK", ""),
        ],
        Box::pin(backend.attach_network("tqa-sbx-1")),
    )
    .await?;
    assert_eq!(subnet?.to_string(), "192.168.4.0/28");
    assert!(seen[0].contains("/networks/create"), "{seen:?}");
    assert_eq!(free(&backend)?, 1);
    Ok(())
}

#[tokio::test]
async fn sbx_u_005_a_failed_gateway_attach_removes_the_network_and_returns_the_subnet() -> TestResult
{
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    let (result, seen) = scripted(
        listener,
        vec![
            step(
                "/networks/create",
                "201 Created",
                r#"{"Id":"n1","Warning":""}"#,
            ),
            step(
                "/networks/tqa-sbx-1/connect",
                "500 Internal Server Error",
                r#"{"message":"boom"}"#,
            ),
            step(
                "GET /networks/tqa-sbx-1",
                "200 OK",
                &inspected("tqa-unit", "192.168.4.0/28", false),
            ),
            step(
                "/networks/tqa-sbx-1/disconnect",
                "403 Forbidden",
                r#"{"message":"not connected"}"#,
            ),
            step("DELETE /networks/tqa-sbx-1", "204 No Content", ""),
        ],
        Box::pin(backend.attach_network("tqa-sbx-1")),
    )
    .await?;
    assert!(
        matches!(result, Err(SandboxError::Backend(_))),
        "{result:?}"
    );
    assert!(seen[2].contains("/networks/tqa-sbx-1"), "{seen:?}");
    assert_eq!(free(&backend)?, 2, "the subnet is back in the pool");
    Ok(())
}

#[tokio::test]
async fn sbx_u_005_a_subnet_docker_calls_overlapping_is_skipped_for_the_next() -> TestResult {
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    let (subnet, _) = scripted(
        listener,
        vec![
            step(
                "/networks/create",
                "403 Forbidden",
                r#"{"message":"Pool overlaps with other one on this address space"}"#,
            ),
            step(
                "/networks/create",
                "201 Created",
                r#"{"Id":"n1","Warning":""}"#,
            ),
            step("/networks/tqa-sbx-1/connect", "200 OK", ""),
        ],
        Box::pin(backend.attach_network("tqa-sbx-1")),
    )
    .await?;
    assert_eq!(subnet?.to_string(), "192.168.4.16/28");
    assert_eq!(free(&backend)?, 0, "one in use, one unusable until restart");
    Ok(())
}

#[tokio::test]
async fn sbx_u_005_other_create_failures_return_the_subnet_and_the_error() -> TestResult {
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    let (result, _) = scripted(
        listener,
        vec![step(
            "/networks/create",
            "500 Internal Server Error",
            r#"{"message":"boom"}"#,
        )],
        Box::pin(backend.attach_network("tqa-sbx-1")),
    )
    .await?;
    assert!(
        matches!(result, Err(SandboxError::Backend(_))),
        "{result:?}"
    );
    assert_eq!(free(&backend)?, 2);
    Ok(())
}

#[tokio::test]
async fn sbx_u_005_an_exhausted_pool_or_a_missing_gateway_asks_docker_nothing() -> TestResult {
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    {
        let mut allocator = backend.allocator.lock().map_err(|_| "allocator")?;
        allocator.allocate().ok_or("first")?;
        allocator.allocate().ok_or("second")?;
    }
    let (result, seen) = scripted(
        listener,
        vec![],
        Box::pin(backend.attach_network("tqa-sbx-1")),
    )
    .await?;
    assert_eq!(result, Err(SandboxError::Capacity));
    assert_eq!(
        result.err().map(|error| (error.code(), error.retryable())),
        Some(("sandbox.network_pool_exhausted", true))
    );
    assert!(seen.is_empty());
    let (backend, listener, _storage) = scripted_backend(None).await?;
    let (result, seen) = scripted(
        listener,
        vec![],
        Box::pin(backend.attach_network("tqa-sbx-1")),
    )
    .await?;
    assert!(
        matches!(result, Err(SandboxError::Preflight(_))),
        "{result:?}"
    );
    assert!(seen.is_empty());
    Ok(())
}

#[tokio::test]
async fn sbx_u_005_detaching_converges_from_every_partial_state() -> TestResult {
    let subnet = Cidr::parse("192.168.4.0/28").ok_or("subnet")?;
    // Already gone: the known subnet is released.
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    backend
        .allocator
        .lock()
        .map_err(|_| "allocator")?
        .allocate();
    let (result, _) = scripted(
        listener,
        vec![step(
            "GET /networks/tqa-sbx-1",
            "404 Not Found",
            r#"{"message":"no such network"}"#,
        )],
        Box::pin(backend.detach_network("tqa-sbx-1", Some(subnet))),
    )
    .await?;
    result?;
    assert_eq!(free(&backend)?, 2);
    // Attached and present: disconnect, remove, and release the subnet from the label.
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    backend
        .allocator
        .lock()
        .map_err(|_| "allocator")?
        .allocate();
    let (result, seen) = scripted(
        listener,
        vec![
            step(
                "GET /networks/tqa-sbx-1",
                "200 OK",
                &inspected("tqa-unit", "192.168.4.0/28", true),
            ),
            step("/networks/tqa-sbx-1/disconnect", "200 OK", ""),
            step(
                "DELETE /networks/tqa-sbx-1",
                "404 Not Found",
                r#"{"message":"gone meanwhile"}"#,
            ),
        ],
        Box::pin(backend.detach_network("tqa-sbx-1", None)),
    )
    .await?;
    result?;
    assert_eq!(seen.len(), 3);
    assert_eq!(free(&backend)?, 2);
    Ok(())
}

#[tokio::test]
async fn sbx_u_005_detach_refuses_a_foreign_network_and_a_failed_disconnect_of_an_attached_gateway()
-> TestResult {
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    let (result, seen) = scripted(
        listener,
        vec![step(
            "GET /networks/tqa-sbx-1",
            "200 OK",
            &inspected("someone-else", "192.168.4.0/28", true),
        )],
        Box::pin(backend.detach_network("tqa-sbx-1", None)),
    )
    .await?;
    assert!(
        matches!(result, Err(SandboxError::Preflight(_))),
        "{result:?}"
    );
    assert_eq!(seen.len(), 1, "nothing is disconnected or removed");
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    let (result, _) = scripted(
        listener,
        vec![
            step(
                "GET /networks/tqa-sbx-1",
                "200 OK",
                &inspected("tqa-unit", "192.168.4.0/28", true),
            ),
            step(
                "/networks/tqa-sbx-1/disconnect",
                "500 Internal Server Error",
                r#"{"message":"boom"}"#,
            ),
        ],
        Box::pin(backend.detach_network("tqa-sbx-1", None)),
    )
    .await?;
    assert!(
        matches!(result, Err(SandboxError::Backend(_))),
        "{result:?}"
    );
    Ok(())
}

#[test]
fn sbx_u_005_network_names_are_short_and_distinct() {
    let name = super::sandbox_network::network_name(1_758_880_000_123_456_789);
    assert_eq!(name, "tqa-sbx-1868cc0a89e5cd15");
    assert!(name.len() <= 63);
    assert_ne!(
        name,
        super::sandbox_network::network_name(1_758_880_000_123_456_790)
    );
}

#[tokio::test]
async fn sbx_u_005_a_network_that_could_not_be_removed_keeps_its_subnet_for_the_sweep() -> TestResult
{
    let (backend, listener, _storage) = scripted_backend(Some("tqa-gateway")).await?;
    let first = backend
        .allocator
        .lock()
        .map_err(|_| "allocator")?
        .allocate()
        .ok_or("first")?;
    let (result, _) = scripted(
        listener,
        vec![
            step(
                "GET /networks/tqa-sbx-1",
                "200 OK",
                &inspected("tqa-unit", &first.to_string(), true),
            ),
            step("/networks/tqa-sbx-1/disconnect", "200 OK", ""),
            step(
                "DELETE /networks/tqa-sbx-1",
                "403 Forbidden",
                r#"{"message":"network has active endpoints"}"#,
            ),
        ],
        Box::pin(backend.detach_network("tqa-sbx-1", None)),
    )
    .await?;
    assert!(result.is_err(), "the failure is reported to the caller");
    assert_eq!(
        free(&backend)?,
        1,
        "the leftover network still holds its subnet"
    );
    // The next sandbox gets a different subnet.
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let next = DockerBackend::unconnected(
        Docker::connect_with_http(&origin, 5, API_DEFAULT_VERSION)?,
        backend.config.clone(),
    );
    *next.allocator.lock().map_err(|_| "allocator")? = std::mem::replace(
        &mut *backend.allocator.lock().map_err(|_| "allocator")?,
        super::pool::Allocator::rebuild(backend.config.network_pool, []).0,
    );
    let (subnet, _) = scripted(
        listener,
        vec![
            step(
                "/networks/create",
                "201 Created",
                r#"{"Id":"n2","Warning":""}"#,
            ),
            step("/networks/tqa-sbx-2/connect", "200 OK", ""),
        ],
        Box::pin(next.attach_network("tqa-sbx-2")),
    )
    .await?;
    assert_ne!(subnet?, first);
    Ok(())
}

#[cfg(test)]
#[path = "create_tests.rs"]
mod create;
