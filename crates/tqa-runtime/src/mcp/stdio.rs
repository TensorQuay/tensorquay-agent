//! One stdio MCP server run by the runtime inside the sandbox (ADR-0011 decisions 3 and 6).
//! The runtime spawns the process itself, so its process group, environment, stderr and kill stay ours, and hands rmcp a
//! bounded transport: a framed reader that ends at the first line over 1 MiB or the first line that isn't JSON.

use std::{
    collections::VecDeque,
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures::StreamExt;
use rmcp::{
    ClientHandler, RoleClient, ServiceError, ServiceExt,
    model::{
        CallToolRequest, CallToolRequestParams, ClientCapabilities, ClientRequest, Implementation,
        InitializeRequestParams, ProtocolVersion, ServerResult,
    },
    service::{MaybeSendFuture, NotificationContext, PeerRequestOptions, RunningService},
    transport::async_rw::JsonRpcMessageCodec,
};
use serde_json::Value;
use tokio::{
    io::AsyncReadExt,
    process::{Child, ChildStderr},
    sync::{Mutex, Semaphore},
};
use tokio_util::{
    codec::{FramedRead, FramedWrite},
    sync::CancellationToken,
};
use tqa_types::{McpServer, McpTransport};

/// The one protocol version this client speaks (ADR-0011 decision 6).
const PROTOCOL: ProtocolVersion = ProtocolVersion::V_2025_11_25;
/// The largest message line either side may send.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// The most tools one server may offer.
pub const MAX_TOOLS_PER_SERVER: usize = 64;
/// Concurrent calls per server; further calls wait.
const CONCURRENT_CALLS: usize = 4;
/// The bounded tail of a server's stderr kept for diagnostics, never logged verbatim.
const STDERR_RING_BYTES: usize = 64 * 1024;
/// How long a server gets to exit at each stage of a stop.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// Why a server could not start or was stopped; each has a stable code of at most 64 bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum McpFailure {
    /// The configured command does not exist in the sandbox.
    #[error("MCP server command not found")]
    CommandNotFound,
    /// The handshake did not finish within its bound.
    #[error("MCP server handshake timed out")]
    HandshakeTimeout,
    /// The server broke the protocol: a line over the bound, non-JSON output, or an early exit.
    #[error("MCP server protocol error")]
    ProtocolError,
    /// The server offered more tools than allowed.
    #[error("MCP server offered too many tools")]
    OverLimit,
    /// The configured transport cannot be served here.
    #[error("MCP transport unavailable")]
    TransportUnavailable,
}
impl McpFailure {
    /// The stable reason code carried by events.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::CommandNotFound => "command_not_found",
            Self::HandshakeTimeout => "handshake_timeout",
            Self::ProtocolError => "protocol_error",
            Self::OverLimit => "over_limit",
            Self::TransportUnavailable => "transport_unavailable",
        }
    }
}

/// Why a call did not return a result.
#[derive(Debug, thiserror::Error)]
pub enum McpCallError {
    /// The call's timeout passed; the server was told to cancel.
    #[error("MCP call timed out")]
    Timeout,
    /// The run was cancelled; the server was told to cancel.
    #[error("MCP call cancelled")]
    Cancelled,
    /// The server exited or closed its output.
    #[error("MCP server exited")]
    Crashed,
    /// The server answered with a protocol error, bounded.
    #[error("MCP protocol error: {0}")]
    Protocol(String),
}

/// A tool as the server listed it; its schema and annotations are untrusted until registered.
#[derive(Clone, Debug)]
pub struct ListedTool {
    /// The server's own tool name.
    pub name: String,
    /// The server's description.
    pub description: String,
    /// The input schema object.
    pub input_schema: Value,
    /// The annotations object, or null.
    pub annotations: Value,
}

/// How a stop ended the server.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stopped {
    /// It exited once its input closed.
    OnInputClosed,
    /// It exited on SIGTERM.
    OnTerminate,
    /// It had to be killed.
    Killed,
}

/// A call's result as text for the model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallOutput {
    /// Text parts, structured content as compact JSON, other parts as a bounded placeholder.
    pub text: String,
    /// Whether the server marked the result as an error.
    pub is_error: bool,
}

#[derive(Clone)]
struct Handler {
    config: InitializeRequestParams,
    stale: Arc<AtomicBool>,
}
impl ClientHandler for Handler {
    fn get_info(&self) -> InitializeRequestParams {
        self.config.clone()
    }
    fn on_tool_list_changed(
        &self,
        _: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + MaybeSendFuture + '_ {
        // A changed list applies at the next run boundary; a run's catalogue stays fixed.
        self.stale.store(true, Ordering::SeqCst);
        std::future::ready(())
    }
}

/// A started stdio server with its discovered tools.
pub struct StdioServer {
    service: RunningService<RoleClient, Handler>,
    child: Mutex<Child>,
    process_group: i32,
    tools: Vec<ListedTool>,
    stale: Arc<AtomicBool>,
    calls: Semaphore,
    stderr: Arc<std::sync::Mutex<VecDeque<u8>>>,
}

impl StdioServer {
    /// Spawns the server, completes the handshake and lists its tools, all within `handshake`.
    ///
    /// # Errors
    /// Returns the typed reason the server could not be used; the process is killed on any failure.
    pub async fn start(
        server: &McpServer,
        workspace: &Path,
        handshake: Duration,
    ) -> Result<Self, McpFailure> {
        let mut child = spawn(server, workspace)?;
        let process_group = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .ok_or(McpFailure::ProtocolError)?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(McpFailure::ProtocolError);
        };
        let stderr_ring = Arc::new(std::sync::Mutex::new(VecDeque::new()));
        tokio::spawn(keep_tail(stderr, stderr_ring.clone()));
        // Bounded both ways; the read side ends at the first framing error (a line too long, or not JSON).
        let read = FramedRead::new(
            stdout,
            JsonRpcMessageCodec::new_with_max_length(MAX_LINE_BYTES),
        )
        .take_while(|item| std::future::ready(item.is_ok()))
        .filter_map(|item| std::future::ready(item.ok()));
        let write = FramedWrite::new(
            stdin,
            JsonRpcMessageCodec::new_with_max_length(MAX_LINE_BYTES),
        );
        let stale = Arc::new(AtomicBool::new(false));
        let handler = Handler {
            config: InitializeRequestParams::new(
                ClientCapabilities::default(),
                Implementation::new("tqa-runtime", env!("CARGO_PKG_VERSION")),
            )
            .with_protocol_version(PROTOCOL),
            stale: stale.clone(),
        };
        let started = tokio::time::timeout(handshake, async {
            let service = handler
                .serve((write, read))
                .await
                .map_err(|_| McpFailure::ProtocolError)?;
            let listed = service
                .peer()
                .list_all_tools()
                .await
                .map_err(|_| McpFailure::ProtocolError)?;
            Ok::<_, McpFailure>((service, listed))
        })
        .await;
        let (service, listed) = match started {
            Ok(Ok(started)) => started,
            Ok(Err(failure)) => return Err(failure),
            Err(_) => return Err(McpFailure::HandshakeTimeout),
        };
        if listed.len() > MAX_TOOLS_PER_SERVER {
            return Err(McpFailure::OverLimit);
        }
        let tools = listed
            .iter()
            .filter_map(|tool| serde_json::to_value(tool).ok())
            .map(|tool| listed_tool(&tool))
            .collect();
        Ok(Self {
            service,
            child: Mutex::new(child),
            process_group,
            tools,
            stale,
            calls: Semaphore::new(CONCURRENT_CALLS),
            stderr: stderr_ring,
        })
    }

    /// The tools the server listed at start.
    #[must_use]
    pub fn tools(&self) -> &[ListedTool] {
        &self.tools
    }

    /// The server's process group, recorded at spawn for the reaper's exclusion.
    #[must_use]
    pub const fn process_group(&self) -> i32 {
        self.process_group
    }

    /// Whether the server announced a changed tool list since it started.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        self.stale.load(Ordering::SeqCst)
    }

    /// The server's own name and version from its handshake.
    #[must_use]
    pub fn server_info(&self) -> Option<(String, String)> {
        let info = self.service.peer().peer_info()?;
        let server = info.server_info.as_ref()?;
        Some((server.name.clone(), server.version.clone()))
    }

    /// The bounded size of the stderr tail kept for diagnostics.
    #[must_use]
    pub fn stderr_len(&self) -> usize {
        self.stderr.lock().map_or(0, |ring| ring.len())
    }

    /// Calls a tool with a timeout and the run's cancellation; both send `notifications/cancelled`.
    ///
    /// # Errors
    /// Returns a timeout, a cancellation, a crash or a bounded protocol error.
    pub async fn call(
        &self,
        tool: &str,
        arguments: Value,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<CallOutput, McpCallError> {
        let _slot = self
            .calls
            .acquire()
            .await
            .map_err(|_| McpCallError::Crashed)?;
        let mut params = CallToolRequestParams::new(tool.to_owned());
        params.arguments = arguments.as_object().cloned();
        let request = ClientRequest::CallToolRequest(CallToolRequest::new(params));
        let handle = self
            .service
            .peer()
            .send_cancellable_request(request, PeerRequestOptions::no_options())
            .await
            .map_err(classify)?;
        let peer = self.service.peer().clone();
        let id = handle.id.clone();
        let outcome = tokio::select! {
            outcome = tokio::time::timeout(timeout, handle.await_response()) => outcome,
            () = cancel.cancelled() => {
                notify_cancelled(&peer, id, "the run was cancelled").await;
                return Err(McpCallError::Cancelled);
            }
        };
        match outcome {
            Err(_) => {
                notify_cancelled(&peer, id, "the call timed out").await;
                Err(McpCallError::Timeout)
            }
            Ok(Err(error)) => Err(classify(error)),
            Ok(Ok(ServerResult::CallToolResult(result))) => {
                Ok(output(&serde_json::to_value(result).unwrap_or_default()))
            }
            Ok(Ok(_)) => Err(McpCallError::Protocol("unexpected response".into())),
        }
    }

    /// Whether the process has exited, without waiting.
    pub async fn exited_now(&self) -> bool {
        matches!(self.child.lock().await.try_wait(), Ok(Some(_)))
    }

    /// Kills the server's whole group at once, when a graceful stop is not possible.
    pub fn kill(&self) {
        signal_group(self.process_group, rustix::process::Signal::KILL);
    }

    /// Whether the process has exited, waiting up to a second for it.
    pub async fn has_exited(&self) -> bool {
        let mut child = self.child.lock().await;
        for _ in 0..50 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    /// Stops the server: close its input, then SIGTERM its group, then SIGKILL, each after a grace period.
    pub async fn stop(self) -> Stopped {
        let _ = self.service.cancel().await;
        let mut child = self.child.lock().await;
        if tokio::time::timeout(STOP_GRACE, child.wait()).await.is_ok() {
            return Stopped::OnInputClosed;
        }
        signal_group(self.process_group, rustix::process::Signal::TERM);
        if tokio::time::timeout(STOP_GRACE, child.wait()).await.is_ok() {
            return Stopped::OnTerminate;
        }
        signal_group(self.process_group, rustix::process::Signal::KILL);
        let _ = child.wait().await;
        Stopped::Killed
    }
}

/// Spawns the server as a tool process (OOM score 500) in its own group, with only its configured environment.
fn spawn(server: &McpServer, workspace: &Path) -> Result<Child, McpFailure> {
    let McpTransport::Stdio { command, args, env } = &server.transport else {
        return Err(McpFailure::TransportUnavailable);
    };
    let mut process =
        tqa_tools::tool_command(command, None).map_err(|_| McpFailure::CommandNotFound)?;
    process
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env(
            "LANG",
            std::env::var_os("LANG").unwrap_or_else(|| "C.UTF-8".into()),
        )
        .env("HOME", workspace.join(".tqa").join("home"))
        .envs(env)
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    process.spawn().map_err(|_| McpFailure::CommandNotFound)
}

pub(super) fn listed_tool(tool: &Value) -> ListedTool {
    ListedTool {
        name: tool["name"].as_str().unwrap_or_default().to_owned(),
        description: tool["description"].as_str().unwrap_or_default().to_owned(),
        input_schema: tool["inputSchema"].clone(),
        annotations: tool["annotations"].clone(),
    }
}

fn signal_group(group: i32, signal: rustix::process::Signal) {
    if let Some(pid) = rustix::process::Pid::from_raw(group) {
        let _ = rustix::process::kill_process_group(pid, signal);
    }
}

async fn notify_cancelled(peer: &rmcp::Peer<RoleClient>, id: rmcp::model::RequestId, reason: &str) {
    let notice = rmcp::model::CancelledNotificationParam::new(Some(id), Some(reason.to_owned()));
    let _ = peer.notify_cancelled(notice).await;
}

fn classify(error: ServiceError) -> McpCallError {
    match error {
        ServiceError::Timeout { .. } => McpCallError::Timeout,
        ServiceError::Cancelled { .. } => McpCallError::Cancelled,
        ServiceError::TransportClosed | ServiceError::TransportSend(_) => McpCallError::Crashed,
        other => McpCallError::Protocol(bounded(&other.to_string())),
    }
}

/// Maps a `CallToolResult` in its wire form to text (ADR-0031).
pub(super) fn output(result: &Value) -> CallOutput {
    let mut parts: Vec<String> = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|part| match part["type"].as_str() {
            Some("text") => part["text"].as_str().unwrap_or_default().to_owned(),
            Some(kind) => {
                let media = part["mimeType"].as_str().unwrap_or(kind);
                format!("[{} content omitted]", bounded(media))
            }
            None => "[content omitted]".to_owned(),
        })
        .collect();
    if let Some(structured) = result
        .get("structuredContent")
        .filter(|value| !value.is_null())
    {
        parts.push(structured.to_string());
    }
    CallOutput {
        text: parts.join("\n"),
        is_error: result["isError"].as_bool() == Some(true),
    }
}

fn bounded(text: &str) -> String {
    text.chars().take(200).collect()
}

async fn keep_tail(mut stderr: ChildStderr, ring: Arc<std::sync::Mutex<VecDeque<u8>>>) {
    let mut buffer = [0_u8; 4096];
    while let Ok(read) = stderr.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        if let Ok(mut ring) = ring.lock() {
            ring.extend(&buffer[..read]);
            let excess = ring.len().saturating_sub(STDERR_RING_BYTES);
            ring.drain(..excess);
        }
    }
}
