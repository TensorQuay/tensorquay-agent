//! The fake deliberately rejects sequence gaps instead of hiding a runtime replay defect.
use super::{MAX_BYTES, Shared, deadline, seq};
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use futures::StreamExt as _;
use std::sync::{Arc, PoisonError};
use tokio::sync::mpsc;
use tqa_types::{ClientFrame, Seq, ServerFrame};

pub(super) async fn upgrade(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !super::http::authorize(&shared, &headers, "/runtime/link") {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    if !shared
        .data
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .link_available
    {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    upgrade
        .max_message_size(tqa_types::MAX_FRAME_BYTES)
        .max_frame_size(tqa_types::MAX_FRAME_BYTES)
        .on_upgrade(move |socket| async move {
            let tracker = shared.connections.clone();
            tracker.track_future(connection(shared, socket)).await;
        })
        .into_response()
}
async fn connection(shared: Arc<Shared>, mut socket: WebSocket) {
    let (send, mut actions) = mpsc::channel(64);
    let cancel = shared.cancel.child_token();
    let path = super::path::current(&shared);
    let first = {
        let mut data = shared.data.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = data.connection.replace(cancel.clone()) {
            previous.cancel();
        }
        data.actions = Some(send);
        data.registrations += 1;
        let mut session = data.session.clone();
        if let ClientFrame::SessionConfig {
            last_stored_seq, ..
        } = &mut session
        {
            *last_stored_seq = data.stored.last().and_then(seq).unwrap_or(Seq::ZERO);
        }
        data.first
            .pop_front()
            .or_else(|| serde_json::to_string(&session).ok())
    };
    shared.changed.notify_waiters();
    let Some(first) = first else {
        return;
    };
    if !write(&mut socket, Message::Text(first.into())).await {
        return;
    }
    let mut silenced = false;
    loop {
        tokio::select! {biased;
            ()=cancel.cancelled()=>break,
            ()=path.cancelled()=>{silenced = true; break;}
            action=actions.recv()=>{
                let Some(text)=action else {break;};
                if !write(&mut socket,Message::Text(text.into())).await {break;}
            }
            message=socket.next()=>match message {
                Some(Ok(Message::Text(text)))=>{if !receive(&shared,text.as_bytes()) {break;}},
                Some(Ok(Message::Binary(bytes)))=>{if !receive(&shared,&bytes) {break;}},
                Some(Ok(Message::Ping(bytes)))=>{if !write(&mut socket,Message::Pong(bytes)).await {break;}},
                Some(Ok(Message::Pong(_)))=>{},
                Some(Ok(Message::Close(_)))=>{
                    shared.data.lock().unwrap_or_else(PoisonError::into_inner).closes += 1;
                    shared.changed.notify_waiters();
                    break;
                }
                _=>break,
            }
        }
    }
    if silenced {
        // A cut path is silent: no pongs, no close frame. The socket stays open, unread, until the path is back (or the
        // gateway stops), then goes without a close, as a dead connection does.
        loop {
            let notified = shared.changed.notified();
            if !super::path::current(&shared).is_cancelled() || shared.cancel.is_cancelled() {
                return;
            }
            tokio::select! {
                () = notified => {}
                () = shared.cancel.cancelled() => return,
            }
        }
    }
    // A test may hold the reply (bounded), to see whether the runtime waits for the close handshake.
    let _ = tokio::time::timeout(deadline(), async {
        loop {
            let notified = shared.changed.notified();
            if !shared.hold_close.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    })
    .await;
    let _ = write(&mut socket, Message::Close(None)).await;
}
async fn write(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(deadline(), socket.send(message)).await,
        Ok(Ok(()))
    )
}
/// A runtime frame: recorded now, or held while the store is stalled. A malformed frame ends the link either way.
fn receive(shared: &Shared, bytes: &[u8]) -> bool {
    if ServerFrame::parse(bytes).is_err() {
        return false;
    }
    {
        let mut data = shared.data.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(held) = data.stalled.as_mut() {
            held.push(bytes.to_vec());
            return true;
        }
    }
    record(shared, bytes)
}
/// Stalls the store, or releases it and records what it held, in order: it stays stalled until nothing is held, so a
/// frame arriving during the release waits behind the others.
pub(super) fn set_stalled(shared: &Shared, stalled: bool) {
    loop {
        let held = {
            let mut data = shared.data.lock().unwrap_or_else(PoisonError::into_inner);
            match (stalled, data.stalled.as_mut()) {
                (true, None) => {
                    data.stalled = Some(Vec::new());
                    break;
                }
                (true, Some(_)) | (false, None) => break,
                (false, Some(held)) if held.is_empty() => {
                    data.stalled = None;
                    break;
                }
                (false, Some(held)) => std::mem::take(held),
            }
        };
        for bytes in held {
            record(shared, &bytes);
        }
    }
    shared.changed.notify_waiters();
}
fn record(shared: &Shared, bytes: &[u8]) -> bool {
    let Ok(frame) = ServerFrame::parse(bytes) else {
        return false;
    };
    let accepted = {
        let mut data = shared.data.lock().unwrap_or_else(PoisonError::into_inner);
        if data.frames.len() >= data.frame_limit
            || bytes.len() > MAX_BYTES.saturating_sub(data.bytes)
        {
            return false;
        }
        data.bytes += bytes.len();
        data.frames.push(frame.clone());
        persist(&mut data, frame)
    };
    shared.changed.notify_waiters();
    accepted
}
fn persist(data: &mut super::Data, frame: ServerFrame) -> bool {
    let Some(number) = seq(&frame) else {
        return true;
    };
    if !data.persist {
        return true;
    }
    let last = data.stored.last().and_then(seq).unwrap_or(Seq::ZERO);
    if last.next() != Some(number) {
        return false;
    }
    data.stored.push(frame);
    true
}
