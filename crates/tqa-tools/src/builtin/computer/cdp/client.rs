//! A `DevTools` protocol client: one WebSocket to the browser's loopback debugging port, each call matched to its answer
//! by id, a bound on every call, and the page's JavaScript dialogs and file choosers, and the order the browser's pages
//! opened in, remembered from their events.
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

use super::super::driver::DriverError;

/// The longest one call waits for its answer.
pub const CALL_BOUND: Duration = Duration::from_secs(10);
/// The largest message read from the browser (a large page's accessibility tree).
const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;

type Pending = Arc<StdMutex<HashMap<u64, Waiting>>>;

/// A call waiting for its answer: the session it went to, whether it runs the page's script, and where its answer
/// goes.
struct Waiting {
    session: Option<String>,
    script: bool,
    sender: oneshot::Sender<Result<Value, Unanswered>>,
}

/// Why a call has no result: the browser refused it (or closed), or a dialog opening on its page paused it.
enum Unanswered {
    Refused(String),
    Paused,
}

/// What the page's events left to remember: an open JavaScript dialog's message, the file input whose chooser the
/// page opened (its backend node id, and whether it takes several files), and the browser's open pages by target id.
#[derive(Default)]
struct Opened {
    dialog: Option<String>,
    chooser: Option<(i64, bool)>,
    pages: Pages,
}

/// The browser's open pages by target id: those open when the client connected, in no order, and those that opened
/// since, in the order they opened.
#[derive(Clone, Default)]
pub(crate) struct Pages {
    at_connect: Vec<String>,
    opened: Vec<String>,
}

impl Pages {
    /// Where a page stands in the order pages opened, the latest highest: a page open at connect below any that opened
    /// since, and one not in the record yet above them all, since every page there was is in it once the client has
    /// connected.
    #[must_use]
    pub(crate) fn rank(&self, id: &str) -> (u8, usize) {
        if let Some(place) = self.opened.iter().position(|page| page == id) {
            (2, place)
        } else if self.at_connect.iter().any(|page| page == id) {
            (1, 0)
        } else {
            (3, 0)
        }
    }

    fn known(&self, id: &str) -> bool {
        self.at_connect
            .iter()
            .chain(&self.opened)
            .any(|page| page == id)
    }
}

/// A connection to the browser.
pub struct Client {
    outgoing: mpsc::Sender<String>,
    pending: Pending,
    next: AtomicU64,
    opened: Arc<StdMutex<Opened>>,
}

impl Client {
    /// Connects to a `ws://127.0.0.1:...` debugging endpoint, and follows the browser's targets so the order its pages
    /// open in is known ([`Self::pages`]). Only loopback is accepted: the browser's port is the sandbox's own.
    ///
    /// # Errors
    /// `Unavailable` for another address or a failed connection; as [`Self::call`] when the browser won't report its
    /// targets.
    pub async fn connect(endpoint: &str) -> Result<Self, DriverError> {
        if !endpoint.starts_with("ws://127.0.0.1:") {
            return Err(DriverError::Unavailable(
                "the browser's endpoint isn't on loopback".into(),
            ));
        }
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_MESSAGE_BYTES));
        let (socket, _) =
            tokio_tungstenite::connect_async_with_config(endpoint, Some(config), false)
                .await
                .map_err(|_| DriverError::Unavailable("couldn't connect to the browser".into()))?;
        let (sink, stream) = socket.split();
        let (outgoing, queue) = mpsc::channel::<String>(64);
        let pending: Pending = Arc::default();
        let opened: Arc<StdMutex<Opened>> = Arc::default();
        tokio::spawn(write(sink, queue));
        tokio::spawn(read(stream, pending.clone(), opened.clone()));
        let client = Self {
            outgoing,
            pending,
            next: AtomicU64::new(1),
            opened,
        };
        // The browser reports each target it has, then each one that opens or closes: the only record of which page
        // opened last, since Target.getTargets keeps no promised order (Chromium 155 listed a window.open first, and
        // four tabs opened A, B, C, D as A, C, D, B). The targets it has come before its answer and in no order of
        // their own either (D, A, B, C), so they are kept as open at connect, unordered.
        client
            .call("Target.setDiscoverTargets", json!({"discover": true}), None)
            .await?;
        if let Ok(mut opened) = client.opened.lock() {
            let at_connect = std::mem::take(&mut opened.pages.opened);
            opened.pages.at_connect = at_connect;
        }
        Ok(client)
    }

    /// One call, on the browser (no session) or on an attached page's session.
    ///
    /// # Errors
    /// `Unavailable` when the browser doesn't answer within [`CALL_BOUND`] or has gone; `Failed` at once for a call
    /// that runs the page's script when the page opens a JavaScript dialog meanwhile ([`Self::dialog`] then says it),
    /// since a paused page never answers it; `Failed` with the protocol's own error otherwise.
    pub async fn call(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value, DriverError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut request = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        let (sender, answer) = oneshot::channel();
        let waiting = Waiting {
            session: session.map(str::to_owned),
            script: matches!(method, "Runtime.evaluate" | "Runtime.callFunctionOn"),
            sender,
        };
        self.pending
            .lock()
            .map_err(|_| DriverError::Unavailable("the connection's state is poisoned".into()))?
            .insert(id, waiting);
        if self.outgoing.send(request.to_string()).await.is_err() {
            self.forget(id);
            return Err(DriverError::Unavailable(
                "the browser's connection has closed".into(),
            ));
        }
        match tokio::time::timeout(CALL_BOUND, answer).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(Unanswered::Paused))) => Err(DriverError::Failed(format!(
                "a dialog opened while the page ran {method}"
            ))),
            Ok(Ok(Err(Unanswered::Refused(message)))) => {
                Err(DriverError::Failed(protocol_reason(method, &message)))
            }
            Ok(Err(_)) => Err(DriverError::Unavailable(
                "the browser's connection has closed".into(),
            )),
            Err(_) => {
                self.forget(id);
                Err(DriverError::Unavailable(format!(
                    "the browser didn't answer {method} in time"
                )))
            }
        }
    }

    /// The open JavaScript dialog's message, if the page has one open.
    #[must_use]
    pub fn dialog(&self) -> Option<String> {
        self.opened
            .lock()
            .ok()
            .and_then(|opened| opened.dialog.clone())
    }

    /// The browser's open pages by target id: those open at connect, and those that opened since in their order.
    #[must_use]
    pub(crate) fn pages(&self) -> Pages {
        self.opened
            .lock()
            .map(|opened| opened.pages.clone())
            .unwrap_or_default()
    }

    /// The file input whose chooser the page opened since the last call, taken: its backend node id and whether it
    /// takes several files.
    #[must_use]
    pub fn take_chooser(&self) -> Option<(i64, bool)> {
        self.opened
            .lock()
            .ok()
            .and_then(|mut opened| opened.chooser.take())
    }

    fn forget(&self, id: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }
}

type Sink = futures::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;
type Stream = futures::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

/// Sends the queued calls in order; when the client is gone, closes the socket so the browser's side ends too.
async fn write(mut sink: Sink, mut queue: mpsc::Receiver<String>) {
    while let Some(text) = queue.recv().await {
        if sink.send(Message::text(text)).await.is_err() {
            return;
        }
    }
    let _ = sink.close().await;
}

/// Routes every answer and event; when the browser goes, every call still waiting fails at once, not at its bound.
async fn read(mut stream: Stream, pending: Pending, opened: Arc<StdMutex<Opened>>) {
    while let Some(Ok(message)) = stream.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(text.as_str()) {
            route(&pending, &opened, &value);
        }
    }
    let waiting: Vec<_> = pending
        .lock()
        .map(|mut waiting| waiting.drain().collect())
        .unwrap_or_default();
    for (_, waiting) in waiting {
        let _ = waiting.sender.send(Err(Unanswered::Refused(
            "the browser closed its connection".into(),
        )));
    }
}

/// An answer to its call, or an event: a dialog opening or closing, a file chooser opening, or a page opening or
/// closing.
fn route(pending: &Pending, opened: &StdMutex<Opened>, value: &Value) {
    if let Some(id) = value["id"].as_u64() {
        let waiting = pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&id));
        if let Some(waiting) = waiting {
            let answer = match value.get("error") {
                Some(error) => Err(Unanswered::Refused(
                    error["message"].as_str().unwrap_or("error").to_owned(),
                )),
                None => Ok(value["result"].clone()),
            };
            let _ = waiting.sender.send(answer);
        }
        return;
    }
    remember(opened, value);
    if value["method"] == "Page.javascriptDialogOpening" {
        // The page is paused until the dialog is answered, and a script call sent to it before its event came would
        // wait out its bound (c17's integration, RT-I-046): each ends now, after the dialog is remembered.
        let paused: Vec<Waiting> = pending
            .lock()
            .map(|mut pending| {
                let ids: Vec<u64> = pending
                    .iter()
                    .filter(|(_, waiting)| {
                        waiting.script && waiting.session.as_deref() == value["sessionId"].as_str()
                    })
                    .map(|(id, _)| *id)
                    .collect();
                ids.iter().filter_map(|id| pending.remove(id)).collect()
            })
            .unwrap_or_default();
        for waiting in paused {
            let _ = waiting.sender.send(Err(Unanswered::Paused));
        }
    }
}

/// What an event leaves to remember.
fn remember(opened: &StdMutex<Opened>, value: &Value) {
    let Ok(mut opened) = opened.lock() else {
        return;
    };
    let params = &value["params"];
    match value["method"].as_str() {
        Some("Page.javascriptDialogOpening") => {
            opened.dialog = Some(params["message"].as_str().unwrap_or("").to_owned());
        }
        Some("Page.javascriptDialogClosed") => opened.dialog = None,
        Some("Page.fileChooserOpened") => {
            opened.chooser = params["backendNodeId"]
                .as_i64()
                .map(|node| (node, params["mode"] == "selectMultiple"));
        }
        Some("Target.targetCreated") if params["targetInfo"]["type"] == "page" => {
            if let Some(id) = params["targetInfo"]["targetId"].as_str()
                && !opened.pages.known(id)
            {
                opened.pages.opened.push(id.to_owned());
            }
        }
        Some("Target.targetDestroyed") => {
            let pages = &mut opened.pages;
            pages
                .at_connect
                .retain(|page| params["targetId"] != page.as_str());
            pages
                .opened
                .retain(|page| params["targetId"] != page.as_str());
        }
        _ => {}
    }
}

/// The protocol's error as a reason with no page text: the method and a short, fixed-shape message.
fn protocol_reason(method: &str, message: &str) -> String {
    let short: String = message.chars().take(120).collect();
    format!("the browser refused {method}: {short}")
}
