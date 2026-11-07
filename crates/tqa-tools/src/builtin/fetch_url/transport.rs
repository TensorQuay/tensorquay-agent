//! CONNECT status is checked before origin TLS or HTTP; only the proxy address is resolved here.
use super::{BODY_BYTES, FetchUrl, HEAD_BYTES, USER_AGENT, io_error, target};
use crate::ToolError;
use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::{
    Request,
    body::{Body, Incoming},
    client::conn::http1,
    header,
};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::{TlsConnector, rustls::pki_types::ServerName};
use url::{Position, Url};

pub(super) struct Page {
    pub bytes: Vec<u8>,
    pub html: bool,
}
pub(super) enum Response {
    Page(Page),
    Redirect(String),
}
trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
type Socket = Box<dyn Stream>;

fn builder() -> http1::Builder {
    let mut builder = http1::Builder::new();
    builder.max_headers(100).max_buf_size(HEAD_BYTES);
    builder
}
async fn secure(tool: &FetchUrl, stream: Socket, host: &str) -> Result<Socket, ToolError> {
    let name = ServerName::try_from(host.to_owned()).map_err(|_| io_error("invalid TLS name"))?;
    let stream = TlsConnector::from(tool.tls.clone())
        .connect(name, stream)
        .await
        .map_err(|_| io_error("TLS connection failed"))?;
    Ok(Box::new(stream))
}
async fn tunnel(tool: &FetchUrl, url: &Url) -> Result<Socket, ToolError> {
    let destination = target(url)?;
    let port = tool
        .proxy
        .url
        .port_or_known_default()
        .ok_or_else(|| io_error("invalid proxy port"))?;
    let socket = crate::net::connect((tool.proxy.host(), port))
        .await
        .map_err(|_| io_error("proxy connection failed"))?;
    let mut socket: Socket = Box::new(socket);
    if tool.proxy.url.scheme() == "https" {
        socket = secure(tool, socket, tool.proxy.host()).await?;
    }
    let (mut sender, connection) = builder()
        .handshake(TokioIo::new(socket))
        .await
        .map_err(|_| io_error("proxy handshake failed"))?;
    let authority = if destination.host().contains(':') {
        format!("[{}]:{}", destination.host(), destination.port())
    } else {
        format!("{}:{}", destination.host(), destination.port())
    };
    let request = Request::builder()
        .method("CONNECT")
        .uri(&authority)
        .header(header::HOST, &authority)
        .header(header::PROXY_AUTHORIZATION, tool.authorization.clone())
        .body(Empty::<Bytes>::new())
        .map_err(|_| io_error("proxy request failed"))?;
    let exchange = async move {
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| io_error("proxy response failed"))?;
        match response.status().as_u16() {
            200 => hyper::upgrade::on(response)
                .await
                .map_err(|_| io_error("proxy tunnel failed")),
            403 => Err(ToolError::EgressDenied {
                target: destination,
            }),
            407 => Err(ToolError::ProxyAuthentication),
            _ => Err(io_error("proxy refused the connection")),
        }
    };
    let drive = async {
        connection
            .with_upgrades()
            .await
            .map_err(|_| io_error("proxy connection failed"))
    };
    let (upgraded, ()) = tokio::try_join!(exchange, drive)?;
    Ok(Box::new(TokioIo::new(upgraded)))
}
pub(super) async fn fetch(tool: &FetchUrl, url: &Url) -> Result<Response, ToolError> {
    let mut socket = tunnel(tool, url).await?;
    if url.scheme() == "https" {
        socket = secure(tool, socket, target(url)?.host()).await?;
    }
    let (mut sender, connection) = builder()
        .handshake(TokioIo::new(socket))
        .await
        .map_err(|_| io_error("HTTP handshake failed"))?;
    let request = Request::builder()
        .method("GET")
        .uri(&url[Position::BeforePath..Position::AfterQuery])
        .header(
            header::HOST,
            &url[Position::BeforeHost..Position::AfterPort],
        )
        .header(header::USER_AGENT, USER_AGENT)
        .header(header::ACCEPT_ENCODING, "identity")
        .header(header::CONNECTION, "close")
        .body(Empty::<Bytes>::new())
        .map_err(|_| io_error("HTTP request failed"))?;
    let exchange = async move {
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| io_error("HTTP response failed"))?;
        drop(sender);
        receive(response).await
    };
    let drive = async {
        connection
            .await
            .map_err(|_| io_error("HTTP connection failed"))
    };
    let (response, ()) = tokio::try_join!(exchange, drive)?;
    Ok(response)
}
async fn receive(response: hyper::Response<Incoming>) -> Result<Response, ToolError> {
    if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
        let location = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .filter(|location| location.len() <= 4096)
            .ok_or_else(|| io_error("invalid redirect location"))?;
        return Ok(Response::Redirect(location.into()));
    }
    if !response.status().is_success() {
        return Err(io_error(&format!(
            "HTTP response status {}",
            response.status().as_u16()
        )));
    }
    if response
        .body()
        .size_hint()
        .upper()
        .is_some_and(|bytes| bytes > BODY_BYTES as u64)
    {
        return Err(ToolError::OutputTooLarge { max: BODY_BYTES });
    }
    let headers = response.headers();
    if headers
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .any(|value| value != "identity")
    {
        return Err(ToolError::Unsupported(
            "compressed responses are not supported".into(),
        ));
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("text/plain")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let html = matches!(mime.as_str(), "text/html" | "application/xhtml+xml");
    if !html
        && !mime.starts_with("text/")
        && !matches!(mime.as_str(), "application/json" | "application/xml")
    {
        return Err(ToolError::Unsupported(
            "binary response content type".into(),
        ));
    }
    let mut body = response.into_body();
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| io_error("response body failed"))?;
        if let Some(data) = frame.data_ref() {
            if data.len() > BODY_BYTES.saturating_sub(bytes.len()) {
                return Err(ToolError::OutputTooLarge { max: BODY_BYTES });
            }
            bytes.extend_from_slice(data);
        }
    }
    Ok(Response::Page(Page { bytes, html }))
}
