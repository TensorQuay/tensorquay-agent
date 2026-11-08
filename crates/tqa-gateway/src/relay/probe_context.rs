//! The model server's own context (a first-hour check): Ollama gives a model a small context unless told otherwise
//! (4096 below 23 GiB of GPU memory, which is most Macs), and it fits an oversized request by dropping its oldest
//! messages without a word, the person's request first. Its OpenAI-compatible endpoint can't ask for more. So on a
//! local route the check asks the server what the model it just answered with runs with: `GET /api/version` says
//! it's Ollama, `GET /api/ps` names the loaded model's `context_length`. Any other server, an unreachable or
//! unexpected answer, or a model not listed reads as unknown, and the check goes on as before: never a failure of its
//! own.
use serde::Serialize;
use std::time::Duration;
use tqa_llm::Route;

/// Tasks need at least this much context: the fixed part of a request (~2.3k tokens with the tool schemas), the output
/// floor, and room for a few file reads before compaction.
pub(super) const MINIMUM: u32 = 16_384;
/// Each read's bound, within the check's own deadline.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// The most bytes of an answer read.
const MAX_ANSWER_BYTES: usize = 64 * 1024;

/// What the server says the route's model runs with.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(super) struct ServerContext {
    /// The server that said so: `ollama`.
    pub server: &'static str,
    /// The context, in tokens, the loaded model runs with.
    pub context_length: u32,
    /// What tasks need at least ([`MINIMUM`]).
    pub minimum: u32,
}

/// The server's root: the route's base URL without its OpenAI-compatible `/v1` (and any query or fragment).
pub(super) fn origin(base_url: &str) -> Option<url::Url> {
    let mut url = url::Url::parse(base_url).ok()?;
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix("/v1").unwrap_or(path).to_owned();
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);
    Some(url)
}

/// The context Ollama runs the route's model with, when the route's server is Ollama and lists the model as loaded.
pub(super) async fn ollama(client: &reqwest::Client, route: &Route) -> Option<ServerContext> {
    let origin = origin(&route.base_url)?;
    let version = read(client, &origin, "api/version").await?;
    version.get("version")?.as_str()?;
    let loaded = read(client, &origin, "api/ps").await?;
    let context_length = listed(&loaded, &route.model)?;
    Some(ServerContext {
        server: "ollama",
        context_length,
        minimum: MINIMUM,
    })
}

/// The loaded model's context in an `/api/ps` answer: by its name or model, exactly or as `<model>:latest` (a name
/// without a tag means `latest` to Ollama).
pub(super) fn listed(answer: &serde_json::Value, model: &str) -> Option<u32> {
    let latest = format!("{model}:latest");
    answer
        .get("models")?
        .as_array()?
        .iter()
        .find(|loaded| {
            ["name", "model"].iter().any(|key| {
                loaded
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|name| name == model || name == latest)
            })
        })?
        .get("context_length")?
        .as_u64()
        .and_then(|length| u32::try_from(length).ok())
        .filter(|length| *length > 0)
}

/// One bounded JSON read below `origin`; `None` for anything but a small JSON answer with status 200.
async fn read(
    client: &reqwest::Client,
    origin: &url::Url,
    path: &str,
) -> Option<serde_json::Value> {
    let mut url = origin.clone();
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .extend(path.split('/'));
    let mut response = client.get(url).timeout(READ_TIMEOUT).send().await.ok()?;
    if response.status() != reqwest::StatusCode::OK {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = tokio::time::timeout(READ_TIMEOUT, response.chunk())
        .await
        .ok()?
        .ok()?
    {
        if body.len() + chunk.len() > MAX_ANSWER_BYTES {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).ok()
}

#[cfg(test)]
#[path = "probe_context_tests.rs"]
mod tests;
