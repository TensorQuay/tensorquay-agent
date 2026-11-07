//! GW-I-007: progress deadlines must not expire a healthy long-lived response.
#[path = "support/fixture.rs"]
mod support;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test(start_paused = true)]
async fn gw_i_007_each_progress_read_gets_its_own_deadline() -> support::TestResult {
    let mut tasks = tokio::task::JoinSet::new();
    // Keep fake time under explicit test control while real loopback I/O is pending.
    // The wall bound only releases a broken fixture; it does not determine assertions.
    tasks.spawn(async {
        let bound = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < bound {
            tokio::task::yield_now().await;
        }
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = tokio::sync::mpsc::channel::<()>(1);
    tasks.spawn(async move {
        let result = serve(listener, receiver).await;
        assert!(
            result.is_ok(),
            "loopback progress fixture failed: {result:?}"
        );
    });
    let mut response = support::stream_client()?
        .get(format!("http://{address}"))
        .send()
        .await?;
    for _ in 0..3 {
        tokio::time::advance(support::DEADLINE / 2).await;
        sender.send(()).await?;
        assert_eq!(
            response.chunk().await?.ok_or("response closed")?.as_ref(),
            b"x\n"
        );
    }
    tasks.shutdown().await;
    Ok(())
}

async fn serve(
    listener: tokio::net::TcpListener,
    mut receiver: tokio::sync::mpsc::Receiver<()>,
) -> std::io::Result<()> {
    let (mut socket, _) = listener.accept().await?;
    let mut request = [0; 4096];
    let length = socket.read(&mut request).await?;
    assert!(length > 0, "client closed before sending its request");
    socket
        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
        .await?;
    while receiver.recv().await.is_some() {
        socket.write_all(b"2\r\nx\n\r\n").await?;
    }
    Ok(())
}
