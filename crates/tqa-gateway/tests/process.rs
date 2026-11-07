//! GW-I-017: the binary healthcheck checks HTTP status and bounds an unresponsive server.
use std::{error::Error, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
type TestResult = Result<(), Box<dyn Error>>;

#[tokio::test]
async fn gw_i_017_binary_healthcheck_is_bounded_and_checks_http_status() -> TestResult {
    for (status, expected) in [(Some(200), true), (Some(503), false), (None, false)] {
        assert_eq!(healthcheck(status).await?, expected);
    }
    Ok(())
}

async fn healthcheck(status: Option<u16>) -> Result<bool, Box<dyn Error>> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_tqa-gateway"))
        .args(["healthcheck", &address.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    // Host loaders can delay a new executable before main. Start the HTTP deadline at its request.
    let (mut socket, _) = tokio::time::timeout(Duration::from_mins(2), async {
        tokio::select! {
            result = listener.accept() => result.map_err(|_| "health listener failed"),
            _ = child.wait() => Err("healthcheck exited before making its HTTP request"),
        }
    })
    .await??;
    let mut bytes = [0; 1024];
    let length = tokio::time::timeout(Duration::from_secs(4), socket.read(&mut bytes)).await??;
    assert!(std::str::from_utf8(&bytes[..length])?.starts_with("GET /healthz "));
    if let Some(status) = status {
        tokio::time::timeout(
            Duration::from_secs(4),
            socket.write_all(
                format!(
                    "HTTP/1.1 {status} Result\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            ),
        )
        .await??;
    }
    // With no response, only the client's two-second timeout can finish this request.
    let result = tokio::time::timeout(Duration::from_secs(4), child.wait()).await??;
    drop(socket);
    Ok(result.success())
}
