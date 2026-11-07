//! One startup load, bounded by the registration watermark rather than a moving gateway tail.
use crate::{RuntimeConfig, RuntimeError, RuntimeLimits, history, time::Time};
use secrecy::ExposeSecret as _;
use tokio_util::sync::CancellationToken;
use tqa_types::{Seq, ServerFrame};
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    items: Vec<serde_json::Value>,
    next_after: Option<Seq>,
}
pub(crate) async fn load(
    config: &RuntimeConfig,
    watermark: Seq,
    limits: RuntimeLimits,
    time: &Time,
    cancel: &CancellationToken,
) -> Result<Vec<ServerFrame>, RuntimeError> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(limits.io_timeout)
        .build()
        .map_err(|_| RuntimeError::Link)?;
    let mut stored = vec![];
    let mut after = Seq::ZERO;
    let mut bytes = 0;
    // Request even the empty first page to authenticate the authoritative history boundary.
    loop {
        let mut url = config.history_url();
        url.query_pairs_mut()
            .append_pair("after", &after.to_string())
            .append_pair("limit", &limits.history_page.to_string());
        let request = client.get(url).bearer_auth(config.token.expose_secret());
        let response = time
            .wait(limits.io_timeout, cancel, request.send())
            .await?
            .map_err(|_| RuntimeError::Link)?;
        if !response.status().is_success() {
            return Err(RuntimeError::Link);
        }
        let body = body(response, limits, time, cancel).await?;
        let page: Page = serde_json::from_slice(&body).map_err(|_| RuntimeError::History)?;
        if page.items.len() > limits.history_page {
            return Err(RuntimeError::Limit);
        }
        let mut page_last = None;
        for value in page.items {
            let encoded = serde_json::to_vec(&value).map_err(|_| RuntimeError::History)?;
            let frame = ServerFrame::parse(&encoded).map_err(|_| RuntimeError::History)?;
            let sequence = history::sequence(&frame).ok_or(RuntimeError::History)?;
            page_last = Some(sequence);
            if sequence > watermark {
                break;
            }
            history::advance(&mut after, &frame)?;
            bytes += encoded.len();
            if stored.len() >= limits.history_frames || bytes > limits.history_bytes {
                return Err(RuntimeError::Limit);
            }
            stored.push(frame);
        }
        if after == watermark {
            return Ok(stored);
        }
        if page_last != Some(after) || page.next_after != Some(after) {
            return Err(RuntimeError::History);
        }
        if page_last.is_none() {
            return Err(RuntimeError::History);
        }
    }
}
async fn body(
    mut response: reqwest::Response,
    limits: RuntimeLimits,
    time: &Time,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, RuntimeError> {
    if response
        .content_length()
        .is_some_and(|length| length > limits.history_page_bytes as u64)
    {
        return Err(RuntimeError::Limit);
    }
    let mut bytes = vec![];
    while let Some(chunk) = time
        .wait(limits.io_timeout, cancel, response.chunk())
        .await?
        .map_err(|_| RuntimeError::Link)?
    {
        if chunk.len() > limits.history_page_bytes.saturating_sub(bytes.len()) {
            return Err(RuntimeError::Limit);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
