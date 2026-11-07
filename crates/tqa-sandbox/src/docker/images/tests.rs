use super::*;
use bollard::{API_DEFAULT_VERSION, Docker};
use std::{io, net::Ipv4Addr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

use super::super::policy_tests::support;

#[tokio::test]
async fn sbx_c_010_missing_local_image_never_requests_a_registry_pull()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let (stop, stopped) = oneshot::channel();
    let storage = tempfile::tempdir()?;
    let backend = DockerBackend::unconnected(
        Docker::connect_with_http(&origin, 5, API_DEFAULT_VERSION)?,
        super::super::DockerBackendConfig::new("local-pin", storage.path())?,
    );
    let mut spec = support::spec()?;
    spec.image = crate::ImageRef::local_id(format!("sha256:{}", "0".repeat(64)))?;
    let (requests, result) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(record_missing(listener, stopped), async {
            let result = backend.ensure_image(&spec, &CancellationToken::new()).await;
            let _ = stop.send(());
            result
        })
    })
    .await?;
    let requests = requests?;
    assert_eq!(result, Err(SandboxError::ImageNotFound));
    assert_eq!(
        requests.len(),
        1,
        "missing local IDs must perform only inspection"
    );
    assert!(requests[0].starts_with("GET "));
    assert!(requests[0].contains("/images/sha256"));
    assert!(requests[0].contains("/json "));
    Ok(())
}

async fn record_missing(
    listener: TcpListener,
    mut stop: oneshot::Receiver<()>,
) -> io::Result<Vec<String>> {
    let mut requests = Vec::new();
    loop {
        let (mut stream, _) = tokio::select! {
            _ = &mut stop => return Ok(requests),
            result = listener.accept() => result?,
        };
        let mut bytes = [0; 4096];
        let count = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut bytes)).await??;
        requests.push(
            String::from_utf8_lossy(&bytes[..count])
                .lines()
                .next()
                .unwrap_or_default()
                .into(),
        );
        if requests.len() > 4 {
            return Err(io::Error::other("unexpected request count"));
        }
        tokio::time::timeout(
            Duration::from_secs(5),
            stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
            ),
        )
        .await??;
    }
}

/// SBX-D-004: the tag check warns only when the tag names another image; a missing tag is quiet, and a failed check
/// never fails the create.
#[tokio::test]
async fn sbx_d_004_the_tag_check_warns_only_when_the_tag_moved()
-> Result<(), Box<dyn std::error::Error>> {
    let pinned = format!("sha256:{}", "1".repeat(64));
    for (response, warns) in [
        (
            answer(
                "200 OK",
                &format!(r#"{{"Id":"sha256:{}"}}"#, "2".repeat(64)),
            ),
            true,
        ),
        (answer("200 OK", &format!(r#"{{"Id":"{pinned}"}}"#)), false),
        (answer("404 Not Found", "{}"), false),
        (
            answer("500 Internal Server Error", r#"{"message":"boom"}"#),
            false,
        ),
    ] {
        let capture = Capture::default();
        let _guard = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_writer(capture.clone())
                .with_ansi(false)
                .with_max_level(tracing::Level::WARN)
                .finish(),
        );
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let storage = tempfile::tempdir()?;
        let backend = DockerBackend::unconnected(
            Docker::connect_with_http(&origin, 5, API_DEFAULT_VERSION)?,
            super::super::DockerBackendConfig::new("tag-check", storage.path())?,
        );
        let mut spec = support::spec()?;
        spec.image = crate::ImageRef::new("example/image:stable", pinned.clone())?;
        let cancel = CancellationToken::new();
        let (served, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                serve_once(listener, response),
                backend.warn_if_tag_moved(&spec, Some(&pinned), &cancel)
            )
        })
        .await?;
        let request = served?;
        assert!(
            request.contains("/images/example/image:stable/json"),
            "{request}"
        );
        let text = capture.text()?;
        assert_eq!(text.contains("image tag has moved"), warns, "{text}");
        if warns {
            assert!(
                text.contains("example/image:stable") && text.contains(&pinned),
                "{text}"
            );
        }
    }
    Ok(())
}

fn answer(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn serve_once(listener: TcpListener, response: String) -> io::Result<String> {
    let (mut stream, _) = listener.accept().await?;
    let mut bytes = [0; 4096];
    let count = stream.read(&mut bytes).await?;
    stream.write_all(response.as_bytes()).await?;
    Ok(String::from_utf8_lossy(&bytes[..count])
        .lines()
        .next()
        .unwrap_or_default()
        .into())
}

#[derive(Clone, Default)]
struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
impl Capture {
    fn text(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(String::from_utf8(
            self.0.lock().map_err(|_| "capture lock")?.clone(),
        )?)
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
impl io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("capture lock"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
