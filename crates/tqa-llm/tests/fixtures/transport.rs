//! Owned loopback faults; no sleeps, external hosts, or unjoined server tasks.
use std::{
    io,
    net::Ipv4Addr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpSocket, TcpStream},
    task::JoinHandle,
};

#[derive(Clone, Copy, Debug)]
pub enum Fault {
    Refused,
    Reset,
    Closed,
    StreamCut,
    Malformed,
    Tls,
    BodyLimit,
}

pub struct Transport {
    pub url: String,
    pub connections: Arc<AtomicUsize>,
    socket: Option<TcpSocket>,
    task: Option<JoinHandle<io::Result<()>>>,
    refusing: bool,
}

impl Transport {
    pub fn new(fault: Fault) -> io::Result<Self> {
        let socket = TcpSocket::new_v4()?;
        socket.bind((Ipv4Addr::LOCALHOST, 0).into())?;
        let scheme = if matches!(fault, Fault::Tls) {
            "https"
        } else {
            "http"
        };
        let mut fixture = Self {
            url: if matches!(fault, Fault::Closed) {
                format!("http://localhost:{}", socket.local_addr()?.port())
            } else {
                format!("{scheme}://{}", socket.local_addr()?)
            },
            refusing: matches!(fault, Fault::Refused),
            connections: Arc::default(),
            socket: Some(socket),
            task: None,
        };
        // Refused: the socket stays bound but doesn't listen, so a connection to its port is refused and the port is
        // never released for another test to take before recovery listens on the same socket.
        if !matches!(fault, Fault::Refused) {
            fixture.listen(Some(fault))?;
        }
        Ok(fixture)
    }

    pub fn recover_refused(&mut self) -> io::Result<()> {
        if self.refusing {
            self.listen(None)?;
            self.refusing = false;
        }
        Ok(())
    }

    fn listen(&mut self, fault: Option<Fault>) -> io::Result<()> {
        let listener = self
            .socket
            .take()
            .ok_or_else(|| io::Error::other("already listening"))?
            .listen(8)?;
        let connections = self.connections.clone();
        self.task = Some(tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(10), serve(listener, fault, connections))
                .await?
        }));
        Ok(())
    }

    pub async fn close(&mut self) -> io::Result<()> {
        if let Some(task) = self.task.take() {
            task.abort();
            match task.await {
                Ok(result) => result?,
                Err(error) if error.is_cancelled() => {}
                Err(error) => return Err(io::Error::other(error)),
            }
        }
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

async fn serve(
    listener: TcpListener,
    fault: Option<Fault>,
    connections: Arc<AtomicUsize>,
) -> io::Result<()> {
    for attempt in 0..2 {
        let (mut socket, _) = listener.accept().await?;
        connections.fetch_add(1, Ordering::SeqCst);
        let fault = if attempt == 0 { fault } else { None };
        if matches!(fault, Some(Fault::Tls)) {
            let mut record = [0; 5];
            socket.read_exact(&mut record).await?;
            let length = usize::from(u16::from_be_bytes([record[3], record[4]]));
            if length > 8192 {
                return Err(io::Error::other("TLS record too large"));
            }
            socket.read_exact(&mut vec![0; length]).await?;
            socket
                .write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n")
                .await?;
            continue;
        }
        if matches!(fault, Some(Fault::Reset)) {
            socket.read_exact(&mut [0; 1]).await?;
            continue;
        }
        read_request(&mut socket).await?;
        match fault {
            Some(Fault::Closed) => socket.shutdown().await?,
            Some(Fault::Malformed) => socket.write_all(b"invalid HTTP response\r\n\r\n").await?,
            Some(Fault::BodyLimit) => {
                socket
                    .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 64\r\n\r\n")
                    .await?;
                socket.write_all(&[b'x'; 64]).await?;
            }
            _ => write_stream(&mut socket, matches!(fault, Some(Fault::StreamCut))).await?,
        }
    }
    Ok(())
}

async fn read_request(socket: &mut TcpStream) -> io::Result<()> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() == 8192 {
            return Err(io::Error::other("request headers too large"));
        }
        bytes.push(socket.read_u8().await?);
    }
    let headers = String::from_utf8_lossy(&bytes);
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>())
        })
        .transpose()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        .unwrap_or(0);
    if length > 8192 {
        return Err(io::Error::other("request body too large"));
    }
    socket.read_exact(&mut vec![0; length]).await?;
    Ok(())
}

async fn write_stream(socket: &mut TcpStream, cut: bool) -> io::Result<()> {
    let text = if cut {
        "shared prefix"
    } else {
        "shared prefix complete"
    };
    let mut body =
        format!("data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{text}\"}}}}]}}\n\n");
    if !cut {
        body.push_str("data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n");
    }
    let length = if cut { body.len() + 100 } else { body.len() };
    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {length}\r\n\r\n{body}").as_bytes()).await?;
    socket.shutdown().await
}
