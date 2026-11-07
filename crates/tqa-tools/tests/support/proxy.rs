//! Strict loopback proxy: every accepted tunnel must carry the runtime bearer.
#![allow(
    dead_code,
    reason = "Shared fixture is compiled separately by integration test binaries."
)]
use std::{
    collections::VecDeque,
    io,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;

pub const TOKEN: &str = "test-runtime-credential";
pub type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Clone)]
pub enum Reply {
    Refuse(u16),
    Page(Vec<u8>),
    Stall,
}
pub fn page(content_type: &str, body: impl AsRef<[u8]>) -> Reply {
    let body = body.as_ref();
    let mut reply = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    reply.extend_from_slice(body);
    Reply::Page(reply)
}

#[derive(Default)]
pub struct Seen {
    pub connect: Vec<String>,
    pub origin: Vec<String>,
}
pub struct Proxy {
    pub address: SocketAddr,
    pub seen: Arc<Mutex<Seen>>,
    cancel: CancellationToken,
    task: JoinHandle<io::Result<()>>,
}
impl Proxy {
    pub async fn start(replies: Vec<Reply>) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let seen = Arc::new(Mutex::new(Seen::default()));
        let cancel = CancellationToken::new();
        let task = tokio::spawn(serve(
            listener,
            replies.into(),
            seen.clone(),
            cancel.clone(),
        ));
        Ok(Self {
            address,
            seen,
            cancel,
            task,
        })
    }
    pub fn url(&self) -> String {
        format!("http://{}", self.address)
    }
    pub async fn finish(mut self) -> TestResult {
        self.cancel.cancel();
        tokio::time::timeout(Duration::from_secs(5), &mut self.task).await???;
        Ok(())
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}
async fn serve(
    listener: TcpListener,
    mut replies: VecDeque<Reply>,
    seen: Arc<Mutex<Seen>>,
    cancel: CancellationToken,
) -> io::Result<()> {
    let mut tasks = JoinSet::new();
    let result = loop {
        tokio::select! { biased;
            () = cancel.cancelled() => break Ok(()),
            result = tasks.join_next(), if !tasks.is_empty() => {
                if let Some(result) = result { result.map_err(io::Error::other)??; }
            }
            incoming = listener.accept(), if tasks.len() < 8 => {
                let (socket, _) = incoming?;
                let reply = replies.pop_front().unwrap_or(Reply::Refuse(502));
                let record = seen.clone();
                tasks.spawn(async move {
                    tokio::time::timeout(Duration::from_secs(10), answer(socket, reply, record)).await.map_err(io::Error::other)?
                });
            }
        }
    };
    tasks.abort_all();
    while let Some(joined) = tasks.join_next().await {
        if let Err(error) = joined
            && !error.is_cancelled()
        {
            return Err(io::Error::other(error));
        }
    }
    result
}
async fn head(socket: &mut TcpStream) -> io::Result<String> {
    let mut bytes = Vec::new();
    while bytes.len() <= 16 * 1024 {
        bytes.push(socket.read_u8().await?);
        if bytes.ends_with(b"\r\n\r\n") {
            return String::from_utf8(bytes).map_err(io::Error::other);
        }
    }
    Err(io::Error::other("fixture request header cap"))
}
async fn answer(mut socket: TcpStream, reply: Reply, seen: Arc<Mutex<Seen>>) -> io::Result<()> {
    let connect = head(&mut socket).await?;
    let expected = format!("proxy-authorization: Bearer {TOKEN}");
    let authorized = connect.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("proxy-authorization")
                && format!("proxy-authorization:{value}") == expected
        })
    });
    seen.lock()
        .map_err(|_| io::Error::other("fixture lock"))?
        .connect
        .push(connect);
    let status = if !authorized {
        407
    } else if let Reply::Refuse(status) = reply {
        status
    } else {
        200
    };
    if status != 200 {
        socket
            .write_all(
                format!(
                    "HTTP/1.1 {status} Refused\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .as_bytes(),
            )
            .await?;
        return Ok(());
    }
    if matches!(reply, Reply::Stall) {
        std::future::pending::<()>().await;
    }
    socket
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    let request = head(&mut socket).await?;
    seen.lock()
        .map_err(|_| io::Error::other("fixture lock"))?
        .origin
        .push(request);
    if let Reply::Page(bytes) = reply
        && let Err(error) = socket.write_all(&bytes).await
        && !matches!(
            error.kind(),
            io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
        )
    {
        return Err(error);
    }
    Ok(())
}

/// The page text inside a `fetch_url` result's untrusted-content block (SEC-034): everything between the opening
/// line and the closing line.
pub fn body(text: &str) -> &str {
    let after_open = text.split_once('\n').map_or("", |(_, rest)| rest);
    after_open.rsplit_once('\n').map_or("", |(body, _)| body)
}
