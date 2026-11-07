//! The public HTTP API test client (FR-26, TK-U-006). The API token is read from a file and is never
//! shown by `Debug`, errors or results; bodies are typed with the protocol crate and bounded.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    fmt,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};
use tqa_types::{ConversationId, RunId, Seq, ServerFrame};

/// Largest accepted token file; a real token is far shorter.
const TOKEN_FILE_BYTES: u64 = 4096;
/// Largest downloaded workspace file; task repositories are small by design.
pub const MAX_DOWNLOAD_BYTES: usize = 8 * 1024 * 1024;

/// A client failure that never carries the token or a response body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientError {
    /// The token file is missing, unreadable, empty, oversized or not one line of visible ASCII.
    TokenFile,
    /// The request could not be sent or its response could not be read.
    Transport,
    /// The gateway answered with this non-success status.
    Status(u16),
    /// The response body did not have the expected shape.
    Decode,
    /// A download exceeded [`MAX_DOWNLOAD_BYTES`].
    TooLarge,
}
impl fmt::Display for ClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokenFile => formatter.write_str("the API token file is unusable"),
            Self::Transport => formatter.write_str("the gateway could not be reached"),
            Self::Status(status) => write!(formatter, "the gateway answered {status}"),
            Self::Decode => formatter.write_str("the gateway response had an unexpected shape"),
            Self::TooLarge => formatter.write_str("the download exceeded its size bound"),
        }
    }
}
impl std::error::Error for ClientError {}

struct Token(String);
impl fmt::Debug for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Token(redacted)")
    }
}

/// A new conversation on an optional existing workspace.
#[derive(Debug, Serialize)]
pub struct NewConversation<'a> {
    /// The display title.
    pub title: &'a str,
    /// The logical model route name.
    pub model_route: &'a str,
    /// The environment template; absent uses the gateway default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<&'a str>,
    /// The profile to make the conversation from; absent makes it without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<&'a str>,
    /// The workspace to bind; absent creates an empty one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<&'a str>,
}

/// One page of the authoritative event log.
#[derive(Debug, Deserialize)]
pub struct EventPage {
    /// Frames in sequence order.
    pub items: Vec<ServerFrame>,
    /// The cursor for the next page; absent when the page is empty.
    pub next_after: Option<Seq>,
}

/// One directory level of a conversation's workspace.
#[derive(Debug, Deserialize)]
pub struct FilePage {
    /// Entries of this page.
    pub items: Vec<FileEntry>,
    /// The cursor for the next page, if any.
    pub next_after: Option<String>,
}

/// A workspace directory entry.
#[derive(Debug, Deserialize)]
pub struct FileEntry {
    /// The entry name within its directory.
    pub name: String,
    /// `file` or `dir`.
    pub kind: String,
    /// The size in bytes.
    pub size: u64,
}

/// A model route as the gateway describes it.
#[derive(Clone, Debug, Deserialize)]
pub struct RouteSummary {
    /// The logical route name.
    pub name: String,
    /// The provider's model identifier.
    pub model: String,
    /// The route's context window, when configured.
    pub context_window: Option<u32>,
}

#[derive(Deserialize)]
struct Items<T> {
    items: Vec<T>,
}

#[derive(Deserialize)]
struct Created<T> {
    id: T,
}

/// A bearer-token client for the public HTTP API.
#[derive(Debug)]
pub struct ApiClient {
    http: reqwest::Client,
    base: String,
    token: Token,
    keys: AtomicU64,
}
impl ApiClient {
    /// Connects to `base_url` (for example `http://127.0.0.1:8090`) with the token stored in `token_file`.
    ///
    /// # Errors
    /// [`ClientError::TokenFile`] for an unusable token file, or [`ClientError::Transport`].
    pub fn new(base_url: &str, token_file: &Path, timeout: Duration) -> Result<Self, ClientError> {
        let token = read_token(token_file)?;
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|_| ClientError::Transport)?;
        Ok(Self {
            http,
            base: format!("{}/api/v1", base_url.trim_end_matches('/')),
            token,
            keys: AtomicU64::new(0),
        })
    }
    /// Creates an empty workspace and returns its identifier.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn create_workspace(&self, name: &str) -> Result<String, ClientError> {
        let created: Created<String> = self.post("/workspaces", &json!({ "name": name })).await?;
        Ok(created.id)
    }
    /// Deletes a task; its sandbox, if any, retires on its idle window.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn delete_conversation(&self, id: &ConversationId) -> Result<(), ClientError> {
        let request = self
            .http
            .delete(format!("{}/conversations/{id}", self.base));
        self.send(request).await.map(drop)
    }
    /// Deletes a workspace with its tasks; `409` while a task's sandbox still runs on it.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn delete_workspace(&self, id: &str) -> Result<(), ClientError> {
        let request = self
            .http
            .delete(format!("{}/workspaces/{id}", self.base))
            .query(&[("with_conversations", "true")]);
        self.send(request).await.map(drop)
    }
    /// Forks a workspace and returns the fork's identifier.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn fork_workspace(&self, id: &str, name: &str) -> Result<String, ClientError> {
        let path = format!("/workspaces/{id}/fork");
        let created: Created<String> = self.post(&path, &json!({ "name": name })).await?;
        Ok(created.id)
    }
    /// Creates a conversation and returns its identifier.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn create_conversation(
        &self,
        request: &NewConversation<'_>,
    ) -> Result<ConversationId, ClientError> {
        let body = serde_json::to_value(request).map_err(|_| ClientError::Decode)?;
        let created: Created<ConversationId> = self.post("/conversations", &body).await?;
        Ok(created.id)
    }
    /// Sends a user message, which starts a run.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn send_message(&self, id: &ConversationId, text: &str) -> Result<(), ClientError> {
        let path = format!("/conversations/{id}/messages");
        self.post_accepted(&path, &json!({ "text": text })).await
    }
    /// Reads up to `limit` frames after `after`.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn events(
        &self,
        id: &ConversationId,
        after: Seq,
        limit: u32,
    ) -> Result<EventPage, ClientError> {
        let query = [("after", after.to_string()), ("limit", limit.to_string())];
        self.get(&format!("/conversations/{id}/events"), &query)
            .await
    }
    /// Opens the event stream, replaying every frame after `after` first; read it with an
    /// [`crate::e2e::sse::SseDecoder`]. `lifetime` replaces the client's request timeout, which
    /// would otherwise end a healthy stream.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn stream(
        &self,
        id: &ConversationId,
        after: Seq,
        lifetime: Duration,
    ) -> Result<reqwest::Response, ClientError> {
        let url = format!("{}/conversations/{id}/events", self.base);
        let query = [
            ("stream", "1".to_owned()),
            ("protocol", tqa_types::PROTOCOL_VERSION.to_string()),
            ("after", after.to_string()),
        ];
        let request = self.http.get(url).query(&query).timeout(lifetime);
        self.send(request.header("accept", "text/event-stream"))
            .await
    }
    /// Lists the model routes visible to this account.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn models(&self) -> Result<Vec<RouteSummary>, ClientError> {
        let page: Items<RouteSummary> = self.get("/models", &[]).await?;
        Ok(page.items)
    }
    /// Cancels a run.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn cancel(&self, id: &ConversationId, run: &RunId) -> Result<(), ClientError> {
        let path = format!("/conversations/{id}/runs/{run}/cancel");
        self.post_accepted(&path, &json!({})).await
    }
    /// Lists one directory level of the conversation's workspace.
    ///
    /// # Errors
    /// Any [`ClientError`].
    pub async fn files(
        &self,
        id: &ConversationId,
        path: &str,
        after: Option<&str>,
    ) -> Result<FilePage, ClientError> {
        let mut query = vec![("path", path.to_owned())];
        query.extend(after.map(|after| ("after", after.to_owned())));
        self.get(&format!("/conversations/{id}/files"), &query)
            .await
    }
    /// Downloads one workspace file, bounded by [`MAX_DOWNLOAD_BYTES`].
    ///
    /// # Errors
    /// [`ClientError::TooLarge`] or any other [`ClientError`].
    pub async fn download(&self, id: &ConversationId, path: &str) -> Result<Vec<u8>, ClientError> {
        let url = format!("{}/conversations/{id}/files/download", self.base);
        let request = self.http.get(url).query(&[("path", path)]);
        let mut response = self.send(request).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ClientError::Transport)? {
            if bytes.len() + chunk.len() > MAX_DOWNLOAD_BYTES {
                return Err(ClientError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, ClientError> {
        let request = self.http.get(format!("{}{path}", self.base)).query(query);
        decode(self.send(request).await?).await
    }
    async fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T, ClientError> {
        decode(self.send(self.write(path, body)).await?).await
    }
    async fn post_accepted(&self, path: &str, body: &Value) -> Result<(), ClientError> {
        self.send(self.write(path, body)).await.map(drop)
    }
    fn write(&self, path: &str, body: &Value) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{}{path}", self.base))
            .header("idempotency-key", self.idempotency_key())
            .json(body)
    }
    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, ClientError> {
        let response = request
            .bearer_auth(&self.token.0)
            .send()
            .await
            .map_err(|_| ClientError::Transport)?;
        let status = response.status();
        if !status.is_success() {
            return Err(ClientError::Status(status.as_u16()));
        }
        Ok(response)
    }
    /// Unique per write within a process and across runs; the driver does not retry writes.
    fn idempotency_key(&self) -> String {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let count = self.keys.fetch_add(1, Ordering::Relaxed);
        format!("tqa-e2e-{}-{nanos}-{count}", std::process::id())
    }
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, ClientError> {
    let bytes = response.bytes().await.map_err(|_| ClientError::Transport)?;
    serde_json::from_slice(&bytes).map_err(|_| ClientError::Decode)
}

fn read_token(path: &Path) -> Result<Token, ClientError> {
    let metadata = std::fs::metadata(path).map_err(|_| ClientError::TokenFile)?;
    if !metadata.is_file() || metadata.len() > TOKEN_FILE_BYTES {
        return Err(ClientError::TokenFile);
    }
    let text = std::fs::read_to_string(path).map_err(|_| ClientError::TokenFile)?;
    let token = text.trim_end_matches(['\n', '\r']);
    if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(ClientError::TokenFile);
    }
    Ok(Token(token.to_owned()))
}

#[cfg(test)]
mod tests;
