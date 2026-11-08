//! `GET /api/v1/channels/{id}/events?stream=1` (ADR-0014 point 11): server-sent events like the conversation
//! stream. A message is `{"type": "channel.message", …}` with `id:` its seq; `Last-Event-ID` (or `after`) replays every
//! later message, then the stream goes live on the channel's wake-up. Ending the channel sends the unsequenced
//! `{"type": "channel.state", "state": "ended"}`.
use super::{api, store};
use crate::{
    GatewayError, InvalidReason,
    app::State,
    auth::{Authz, Role, check_origin},
};
use axum::{
    extract::{Path, RawQuery, State as AppState},
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Response, Sse, sse::Event, sse::KeepAlive},
};
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::watch;
use tqa_types::{ChannelFilter, ChannelMessage};
use uuid::Uuid;

/// How many messages one refill reads.
const REFILL: u32 = 200;

fn invalid(field: &'static str) -> GatewayError {
    GatewayError::Invalid {
        field,
        reason: InvalidReason::ParameterInvalid,
    }
}

/// `stream=1` is required; the replay starts after `Last-Event-ID`, else after `after`, else from the start.
fn start(raw: Option<&str>, headers: &HeaderMap) -> Result<u64, GatewayError> {
    let mut stream = false;
    let mut after = 0;
    for (key, value) in url::form_urlencoded::parse(raw.unwrap_or("").as_bytes()) {
        match key.as_ref() {
            "stream" if value == "1" => stream = true,
            "after" => after = value.parse().map_err(|_| invalid("after"))?,
            "stream" => return Err(invalid("stream")),
            _ => return Err(invalid("query")),
        }
    }
    if !stream {
        return Err(invalid("stream"));
    }
    let mut values = headers.get_all("last-event-id").iter();
    let (Some(value), None) = (values.next(), values.next()) else {
        return Ok(after);
    };
    value
        .to_str()
        .ok()
        .filter(|text| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|text| text.parse().ok())
        .ok_or(GatewayError::BadRequest(
            "Last-Event-ID must be a decimal sequence",
        ))
}

/// Where a stream is: what to send next, how far it has read, and whether it has said the channel ended.
struct Cursor {
    state: Arc<State>,
    channel: Uuid,
    caller: (Uuid, bool),
    since: u64,
    pending: VecDeque<ChannelMessage>,
    woken: watch::Receiver<u64>,
    ended_sent: bool,
}

impl Cursor {
    /// The next event, waiting for one; `None` ends the stream (the caller lost sight of the channel, the store
    /// failed, or the gateway stops).
    async fn next(&mut self) -> Option<Event> {
        loop {
            if let Some(message) = self.pending.pop_front() {
                let mut data = serde_json::to_value(&message).ok()?;
                data.as_object_mut()?
                    .insert("type".into(), "channel.message".into());
                return Some(
                    Event::default()
                        .id(message.seq.to_string())
                        .data(data.to_string()),
                );
            }
            let (ended, page) = self.refill().await.ok()?;
            if !page.is_empty() {
                self.since = page.last().map_or(self.since, |message| message.seq);
                self.pending.extend(page);
                continue;
            }
            if ended && !self.ended_sent {
                self.ended_sent = true;
                return Some(Event::default().data(r#"{"type":"channel.state","state":"ended"}"#));
            }
            tokio::select! {
                biased;
                () = self.state.cancel.cancelled() => return None,
                changed = self.woken.changed() => {
                    if changed.is_err() {
                        self.woken = self.state.channels.subscribe(self.channel);
                    }
                }
            }
        }
    }

    async fn refill(&self) -> Result<(bool, Vec<ChannelMessage>), GatewayError> {
        crate::io::bounded(self.state.config.io_timeout(), &self.state.cancel, async {
            let mut connection = self.state.pool.acquire().await?;
            let seen = store::visible(&mut connection, self.caller, self.channel).await?;
            let page = store::read(
                &mut connection,
                self.channel,
                (self.since, &ChannelFilter::default(), REFILL),
            )
            .await?;
            Ok((seen.channel.ended, page.messages))
        })
        .await
    }
}

/// The channel's event stream.
///
/// # Errors
/// `404` when the caller may not see it, `400` for a query without `stream=1` or a bad `Last-Event-ID`, `403` for a
/// foreign origin, storage failure.
pub(crate) async fn events(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    let id = api::channel_id(&id)?;
    check_origin(&headers, &state.config.public_origin, false)?;
    let since = start(raw.as_deref(), &headers)?;
    let caller = (user.id, user.role == Role::Admin);
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        store::visible(&mut connection, caller, id)
            .await
            .map(|_| ())
    })
    .await?;
    // Subscribed before the first read, so nothing committed in between is missed.
    let woken = state.channels.subscribe(id);
    let interval = state.config.stream_keepalive;
    let cursor = Cursor {
        state,
        channel: id,
        caller,
        since,
        pending: VecDeque::new(),
        woken,
        ended_sent: false,
    };
    let stream = futures::stream::unfold(cursor, |mut cursor| async {
        let event = cursor.next().await?;
        Some((Ok::<_, std::convert::Infallible>(event), cursor))
    });
    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(interval).text("keep-alive"))
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}
