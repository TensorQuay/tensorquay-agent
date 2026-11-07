//! A gateway connector reached through the gateway's MCP proxy (ADR-0031): MCP streamable
//! HTTP to `<gateway origin>/mcp/<connector>` with the runtime token, a bounded JSON-RPC client of our own. The URL is
//! derived here, never taken from the wire. Every `tools/call` names its journalled call in the `tqa-*` headers, without
//! which the proxy refuses ask-class calls. Failures keep the proxy's stable codes (§5a).

use std::{
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures::StreamExt;
use reqwest::{Response, StatusCode, header::HeaderMap};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tqa_tools::ToolError;
use url::Url;

use super::{
    CallOutput, ListedTool, McpFailure,
    stdio::{MAX_LINE_BYTES, MAX_TOOLS_PER_SERVER, output},
};

/// The one protocol version this client speaks (ADR-0011 decision 6).
const PROTOCOL: &str = "2025-11-25";
/// The proxy's session header (ADR-0031 decision 4).
const SESSION_HEADER: &str = "mcp-session-id";
/// The longest wait for a best-effort notice to the proxy: a cancellation or the session's DELETE. A stalled proxy then
/// cannot keep a timed-out call, or shutdown, waiting.
pub const NOTICE_BOUND: Duration = Duration::from_secs(5);
/// Concurrent calls per connector, matching the proxy's per-(conversation, connector) bound.
const CONCURRENT_CALLS: usize = 4;

/// Why one exchange with the proxy failed.
#[derive(Debug)]
enum Failure {
    /// The proxy forgot the session (a restart or the idle expiry): initialise again.
    SessionGone,
    /// A coded failure for the model.
    Coded(ToolError),
    /// The connector's own JSON-RPC error.
    Upstream {
        /// The stable code.
        code: String,
        /// The connector's message, bounded.
        message: String,
    },
}

/// A failed connector call.
#[derive(Debug)]
pub enum CallFailure {
    /// The runtime's or the proxy's own failure, coded for the model.
    Coded(ToolError),
    /// A JSON-RPC error answer. The proxy relays a connector's errors verbatim, so its message is the connector's text
    /// (SEC-040).
    Upstream {
        /// The stable code.
        code: String,
        /// The message, at most 200 characters.
        message: String,
    },
}

impl CallFailure {
    /// The failure as the model gets it: an error answer's message goes in an untrusted-content block naming the
    /// registry's `server` and `tool`, and its code stays outside.
    #[must_use]
    pub fn into_tool_error(self, server: &str, tool: &str) -> ToolError {
        match self {
            Self::Coded(error) => error,
            Self::Upstream { code, message } => {
                ToolError::mcp(&code, &super::tool::mark(server, tool, &message))
            }
        }
    }
}

impl From<ToolError> for CallFailure {
    fn from(error: ToolError) -> Self {
        Self::Coded(error)
    }
}

/// A connector reached through the gateway's MCP proxy.
pub struct GatewayServer {
    client: reqwest::Client,
    url: Url,
    token: SecretString,
    session: Mutex<Option<String>>,
    next_id: AtomicU64,
    tools: Vec<ListedTool>,
    calls: Semaphore,
    call_timeout: Duration,
}

impl GatewayServer {
    /// Connects to `<origin>/mcp/<connector>`: initialise, then list the tools, within `handshake`.
    ///
    /// # Errors
    /// Returns `ProtocolError` when the proxy cannot be reached or answers badly, `OverLimit` for too many tools, and
    /// `HandshakeTimeout` past the bound.
    pub async fn connect(
        origin: &Url,
        connector: &str,
        token: SecretString,
        handshake: Duration,
        call_timeout: Duration,
    ) -> Result<Self, McpFailure> {
        let mut url = origin.clone();
        url.set_path(&format!("/mcp/{connector}"));
        url.set_query(None);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(handshake)
            .build()
            .map_err(|_| McpFailure::ProtocolError)?;
        let mut server = Self {
            client,
            url,
            token,
            session: Mutex::new(None),
            next_id: AtomicU64::new(1),
            tools: Vec::new(),
            calls: Semaphore::new(CONCURRENT_CALLS),
            call_timeout,
        };
        let listed = tokio::time::timeout(handshake, async {
            server
                .initialize()
                .await
                .map_err(|_| McpFailure::ProtocolError)?;
            server.list().await
        })
        .await
        .map_err(|_| McpFailure::HandshakeTimeout)??;
        if listed.len() > MAX_TOOLS_PER_SERVER {
            return Err(McpFailure::OverLimit);
        }
        server.tools = listed;
        Ok(server)
    }

    /// The connector's tools as the proxy listed them (already filtered by the connector's list).
    #[must_use]
    pub fn tools(&self) -> &[ListedTool] {
        &self.tools
    }

    /// Calls a tool within the connector's call timeout; a lost session is initialised once more and the call retried.
    ///
    /// # Errors
    /// Returns the proxy's coded failure, `Timeout` or `Cancelled` (both tell the proxy to cancel), or a JSON-RPC
    /// error answer, whose message is the connector's text.
    pub async fn call(
        &self,
        tool: &str,
        arguments: Value,
        cancel: &CancellationToken,
    ) -> Result<CallOutput, CallFailure> {
        let _slot = self
            .calls
            .acquire()
            .await
            .map_err(|_| ToolError::Cancelled)?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":tool,"arguments":arguments}});
        let headers = HeaderMap::new();
        let exchange = async {
            match self.request(&request, Some(id), &headers).await {
                Err(Failure::SessionGone) => {
                    self.initialize().await.map_err(failed)?;
                    self.request(&request, Some(id), &headers)
                        .await
                        .map_err(failed)
                }
                other => other.map_err(failed),
            }
        };
        let answer = tokio::select! {
            answer = tokio::time::timeout(self.call_timeout, exchange) => answer,
            () = cancel.cancelled() => {
                self.notify_cancelled(id, "the run was cancelled").await;
                return Err(ToolError::Cancelled.into());
            }
        };
        let Ok(answer) = answer else {
            self.notify_cancelled(id, "the call timed out").await;
            return Err(ToolError::Timeout.into());
        };
        Ok(output(&answer?.unwrap_or_default()))
    }

    /// Ends the proxy session (ADR-0031 decision 4), best effort and bounded by [`NOTICE_BOUND`].
    pub async fn close(&self) {
        let session = self
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(session) = session {
            let delete = self
                .client
                .delete(self.url.clone())
                .bearer_auth(self.token.expose_secret())
                .header(SESSION_HEADER, session)
                .send();
            let _ = tokio::time::timeout(NOTICE_BOUND, delete).await;
        }
    }

    async fn initialize(&self) -> Result<(), Failure> {
        self.session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{
            "protocolVersion":PROTOCOL,"capabilities":{},
            "clientInfo":{"name":"tqa-runtime","version":env!("CARGO_PKG_VERSION")}}});
        self.request(&request, Some(id), &HeaderMap::new()).await?;
        let initialized = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        self.request(&initialized, None, &HeaderMap::new()).await?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<ListedTool>, McpFailure> {
        let mut tools = Vec::new();
        let mut cursor: Option<Value> = None;
        loop {
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);
            let mut params = json!({});
            if let Some(cursor) = cursor.take() {
                params["cursor"] = cursor;
            }
            let request = json!({"jsonrpc":"2.0","id":id,"method":"tools/list","params":params});
            let result = self
                .request(&request, Some(id), &HeaderMap::new())
                .await
                .map_err(|_| McpFailure::ProtocolError)?
                .unwrap_or_default();
            let page = result["tools"].as_array().map_or(&[][..], Vec::as_slice);
            if tools.len() + page.len() > MAX_TOOLS_PER_SERVER {
                return Err(McpFailure::OverLimit);
            }
            tools.extend(page.iter().map(super::stdio::listed_tool));
            match result.get("nextCursor").filter(|next| !next.is_null()) {
                Some(next) => cursor = Some(next.clone()),
                None => return Ok(tools),
            }
        }
    }

    async fn notify_cancelled(&self, id: u64, reason: &str) {
        let notice = json!({"jsonrpc":"2.0","method":"notifications/cancelled",
            "params":{"requestId":id,"reason":reason}});
        let _ = tokio::time::timeout(NOTICE_BOUND, self.request(&notice, None, &HeaderMap::new()))
            .await;
    }

    /// Sends one JSON-RPC message; for a request, returns the matching response's result.
    async fn request(
        &self,
        message: &Value,
        id: Option<u64>,
        extra: &HeaderMap,
    ) -> Result<Option<Value>, Failure> {
        let session = self
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut builder = self
            .client
            .post(self.url.clone())
            .bearer_auth(self.token.expose_secret())
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", PROTOCOL)
            .headers(extra.clone())
            .json(message);
        if let Some(session) = &session {
            builder = builder.header(SESSION_HEADER, session);
        }
        let response = builder.send().await.map_err(|_| {
            Failure::Coded(ToolError::mcp(
                "connector_unavailable",
                "the gateway's MCP proxy is unreachable",
            ))
        })?;
        if let Some(assigned) = response
            .headers()
            .get(SESSION_HEADER)
            .and_then(|value| value.to_str().ok())
        {
            *self.session.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(assigned.to_owned());
        }
        let status = response.status();
        if status == StatusCode::NOT_FOUND && session.is_some() {
            return Err(Failure::SessionGone);
        }
        if !status.is_success() {
            return Err(Failure::Coded(http_failure(&response)));
        }
        let Some(id) = id else { return Ok(None) };
        if status == StatusCode::ACCEPTED {
            return Err(Failure::Coded(ToolError::mcp(
                "mcp_error",
                "the proxy accepted a request without answering it",
            )));
        }
        let event_stream = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|kind| kind.starts_with("text/event-stream"));
        let answer = if event_stream {
            sse_answer(response, id).await?
        } else {
            let body = bounded_body(response).await?;
            serde_json::from_slice(&body).map_err(|_| malformed())?
        };
        rpc_result(&answer).map(Some)
    }
}

/// A proxy HTTP error with its stable code; `rate_limited` names its Retry-After. The code comes only from the
/// gateway's own [`tqa_types::GATEWAY_ERROR_HEADER`], never the body, so an answer the proxy forwards can't pose as
/// one of the gateway's codes; without the header it is `mcp_error`.
fn http_failure(response: &Response) -> ToolError {
    let status = response.status();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let code = response
        .headers()
        .get(tqa_types::GATEWAY_ERROR_HEADER)
        .and_then(|value| value.to_str().ok())
        .and_then(tqa_types::gateway_error_code)
        .unwrap_or("mcp_error");
    let detail = match retry_after {
        Some(seconds) => format!(
            "the gateway's MCP proxy answered {}; retry after {seconds} s",
            status.as_u16()
        ),
        None => format!("the gateway's MCP proxy answered {}", status.as_u16()),
    };
    ToolError::mcp(code, &detail)
}

/// A JSON-RPC response's result, or its error with a stable code (§5a).
fn rpc_result(answer: &Value) -> Result<Value, Failure> {
    let Some(error) = answer.get("error") else {
        return Ok(answer["result"].clone());
    };
    let message: String = error["message"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect();
    let code = match error["code"].as_i64() {
        Some(-32601) => "mcp_method_not_found".to_owned(),
        Some(-32602) => "mcp_invalid_params".to_owned(),
        _ => "mcp_error".to_owned(),
    };
    Err(Failure::Upstream { code, message })
}

/// The whole body, refused past one message's bound.
async fn bounded_body(response: Response) -> Result<Vec<u8>, Failure> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| malformed())?;
        if body.len() + chunk.len() > MAX_LINE_BYTES {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Reads server-sent events until the response to `id`; each event's data is bounded like a stdio line.
async fn sse_answer(response: Response, id: u64) -> Result<Value, Failure> {
    let mut pending = Vec::new();
    let mut data = String::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        pending.extend_from_slice(&chunk.map_err(|_| malformed())?);
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            // A whole line is bounded too: one whose newline arrives in the chunk that crosses the bound never waits in
            // `pending` long enough for the check below.
            if line.len() > MAX_LINE_BYTES {
                return Err(too_large());
            }
            if let Some(message) = sse_line(line, &mut data, id) {
                return Ok(message);
            }
            if data.len() > MAX_LINE_BYTES {
                return Err(too_large());
            }
        }
        if pending.len() > MAX_LINE_BYTES {
            return Err(too_large());
        }
    }
    Err(Failure::Coded(ToolError::mcp(
        "mcp_error",
        "the proxy's event stream ended without an answer",
    )))
}

/// Feeds one SSE line: a blank line ends an event, which is returned when it answers `id`; a `data:` line adds to it.
fn sse_line(line: &str, data: &mut String, id: u64) -> Option<Value> {
    if line.is_empty() {
        let message = serde_json::from_str::<Value>(data)
            .ok()
            .filter(|message| message["id"].as_u64() == Some(id));
        data.clear();
        return message;
    }
    if let Some(payload) = line.strip_prefix("data:") {
        if !data.is_empty() {
            data.push('\n');
        }
        data.push_str(payload.strip_prefix(' ').unwrap_or(payload));
    }
    None
}

fn failed(failure: Failure) -> CallFailure {
    match failure {
        Failure::Coded(error) => CallFailure::Coded(error),
        Failure::Upstream { code, message } => CallFailure::Upstream { code, message },
        Failure::SessionGone => {
            ToolError::mcp("mcp_error", "the proxy session was lost again").into()
        }
    }
}
fn malformed() -> Failure {
    Failure::Coded(ToolError::mcp(
        "mcp_error",
        "the proxy's answer was malformed",
    ))
}
fn too_large() -> Failure {
    Failure::Coded(ToolError::mcp(
        "mcp_error",
        "the proxy's answer passed the 1 MiB message bound",
    ))
}
