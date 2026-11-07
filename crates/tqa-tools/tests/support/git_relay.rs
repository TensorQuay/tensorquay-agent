//! A finite HTTP script with complete requests, disconnect witnesses and no detached tasks.
use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tqa_tools::ProxyEndpoint;

pub type TestResult = Result<(), Box<dyn std::error::Error>>;
pub const TOKEN: &str = "test-runtime-credential";

pub enum Reply {
    Bytes(Vec<u8>),
    Close,
    Stall,
}
pub fn json_reply(status: u16, body: impl AsRef<[u8]>) -> Reply {
    let body = body.as_ref();
    let mut bytes = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body);
    Reply::Bytes(bytes)
}
#[derive(Clone)]
pub struct SeenRequest {
    pub head: String,
    pub body: Vec<u8>,
}
#[derive(Default)]
struct State {
    requests: Mutex<Vec<SeenRequest>>,
    arrived: Notify,
    disconnected: AtomicUsize,
}
pub struct RelayServer {
    pub endpoint: ProxyEndpoint,
    state: Arc<State>,
    cancel: CancellationToken,
    task: JoinHandle<io::Result<()>>,
}
impl RelayServer {
    pub async fn start(replies: Vec<Reply>) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let endpoint = ProxyEndpoint::new(&format!("http://{}", listener.local_addr()?))?;
        let state = Arc::new(State::default());
        let cancel = CancellationToken::new();
        let task = tokio::spawn(serve(
            listener,
            replies.into(),
            state.clone(),
            cancel.clone(),
        ));
        Ok(Self {
            endpoint,
            state,
            cancel,
            task,
        })
    }
    pub fn requests(&self) -> Vec<SeenRequest> {
        self.state
            .requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    pub async fn arrived(&self) {
        self.state.arrived.notified().await;
    }
    pub async fn finish(mut self) -> Result<usize, Box<dyn std::error::Error>> {
        self.cancel.cancel();
        tokio::time::timeout(Duration::from_secs(10), &mut self.task).await???;
        Ok(self.state.disconnected.load(Ordering::SeqCst))
    }
}
impl Drop for RelayServer {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.task.abort();
    }
}
async fn serve(
    listener: TcpListener,
    mut replies: VecDeque<Reply>,
    state: Arc<State>,
    cancel: CancellationToken,
) -> io::Result<()> {
    loop {
        let (socket, _) = tokio::select! {
            () = cancel.cancelled() => return Ok(()),
            incoming = listener.accept() => incoming?,
        };
        let reply = replies
            .pop_front()
            .ok_or_else(|| io::Error::other("unexpected retry or request"))?;
        tokio::time::timeout(Duration::from_secs(10), answer(socket, reply, &state)).await??;
    }
}
async fn answer(mut socket: TcpStream, reply: Reply, state: &State) -> io::Result<()> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() == 16 * 1024 {
            return Err(io::Error::other("request header bound"));
        }
        head.push(socket.read_u8().await?);
    }
    let head = String::from_utf8(head).map_err(io::Error::other)?;
    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
        .map_or(Ok(0), |(_, value)| {
            value.trim().parse::<usize>().map_err(io::Error::other)
        })?;
    if length > 16 * 1024 {
        return Err(io::Error::other("request body bound"));
    }
    let mut body = vec![0; length];
    socket.read_exact(&mut body).await?;
    state
        .requests
        .lock()
        .map_err(|_| io::Error::other("fixture lock"))?
        .push(SeenRequest { head, body });
    state.arrived.notify_one();
    match reply {
        Reply::Bytes(bytes) => {
            if let Err(error) = socket.write_all(&bytes).await
                && !matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
                )
            {
                return Err(error);
            }
        }
        Reply::Stall => match socket.read_u8().await {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
                ) =>
            {
                state.disconnected.fetch_add(1, Ordering::SeqCst);
            }
            _ => return Err(io::Error::other("cancelled client did not disconnect")),
        },
        Reply::Close => {}
    }
    Ok(())
}
