//! The live Screen panel's socket (ADR-0039 sections 5 and 6): `GET /api/v1/conversations/{id}/screen`, the owner's
//! viewer, paired with a bridge of its own from the sandbox and relayed both ways. Without a session or with a wrong
//! `Origin` it's refused before the upgrade; every other refusal upgrades and closes at once with an application close
//! code, since a page can't read a refused upgrade. A viewer that loses its picture is told why: `sandbox_not_running`
//! when the task has no sandbox, `screen_unavailable` when a bridge dropped or never came (the panel retries). What
//! the viewer sends passes the client filter (view-only unless it holds Take over); the gateway's text frames say who
//! holds the screen. Take over cancels the active run as Stop does and holds every queued turn until the hand-back,
//! which wakes them.
use super::{
    SCREENS,
    record::{Notice, record},
    rfb::ClientFilter,
    viewers::{Holder, ViewerId, next_viewer},
};
use crate::{GatewayError, app::State, auth::Authz, auth::User, auth::check_origin};
use axum::{
    extract::{
        Path,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::Response,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tqa_types::{ClientFrame, ConversationId, ScreenEnded};

/// How long a viewer waits for the runtime's bridge before it's told why none came.
const BRIDGE_WAIT: Duration = Duration::from_secs(35);

/// `GET /api/v1/conversations/{id}/screen` (WebSocket).
///
/// # Errors
/// `401` without a session and `403` with a wrong `Origin`, before the upgrade; everything else is a close code.
pub(crate) async fn view(
    axum::extract::State(state): axum::extract::State<Arc<State>>,
    Authz(user): Authz,
    Path(id): Path<ConversationId>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, GatewayError> {
    check_origin(&headers, &state.config.public_origin, false)?;
    Ok(upgrade.on_upgrade(move |socket| serve(state, user, id, socket)))
}

/// Closes `socket` with an application code and the error code as its reason.
async fn close(mut socket: WebSocket, code: u16, reason: &'static str) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })))
        .await;
}

/// The refusal a viewer gets after the upgrade, if any: `4403 forbidden`, `4404 not_found`, `4409 no_desktop` or
/// `4409 sandbox_not_running`.
async fn refusal(state: &State, user: &User, id: &ConversationId) -> Option<(u16, &'static str)> {
    match crate::conversations::authorize(state, user, id, false).await {
        Err(GatewayError::Forbidden(_)) => return Some((4403, "forbidden")),
        Err(GatewayError::NotFound) => return Some((4404, "not_found")),
        Err(_) => return Some((1011, "store_unavailable")),
        Ok(_) => {}
    }
    match super::declares_desktop(state, id).await {
        Ok(true) => {}
        Ok(false) => return Some((4409, "no_desktop")),
        Err(_) => return Some((1011, "store_unavailable")),
    }
    match running(state, id).await {
        Ok(true) => None,
        Ok(false) => Some((4409, "sandbox_not_running")),
        Err(_) => Some((1011, "store_unavailable")),
    }
}

/// Whether the task's desktop is running: its runtime's link is up, or its sandbox is starting or live. The link alone
/// can't say: it is down for a moment while the runtime redials (within seconds of a gateway restart), and a viewer that
/// came first was told the desktop wasn't running, which the page doesn't retry by itself (WEB-E2E-073, GW-I-274).
async fn running(state: &State, id: &ConversationId) -> Result<bool, GatewayError> {
    if state.links.connected(id) {
        return Ok(true);
    }
    let id = crate::conversations::uuid(id)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT EXISTS (SELECT 1 FROM sandbox_instances WHERE conversation_id=$1 AND phase IN ('starting','live')) AS \"live!\"",
            id
        )
        .fetch_one(&state.pool)
        .await?)
    })
    .await
}

async fn serve(state: Arc<State>, user: User, id: ConversationId, socket: WebSocket) {
    if let Some((code, reason)) = refusal(&state, &user, &id).await {
        return close(socket, code, reason).await;
    }
    let viewer = next_viewer();
    let opened = SCREENS.with(&id, |screen| screen.open(viewer));
    let Some(Ok((paired, control))) = opened else {
        return close(socket, 4409, "screen_viewers").await;
    };
    let bridge = tokio::time::timeout(BRIDGE_WAIT, paired).await;
    let Ok(Ok(bridge)) = bridge else {
        leave(&state, &user, &id, viewer).await;
        return close(socket, 4409, unavailable(&state, &id).await).await;
    };
    let ended = relay(&state, &user, &id, (socket, bridge), (viewer, control)).await;
    SCREENS.with(&id, super::viewers::Screen::bridge_closed);
    leave(&state, &user, &id, viewer).await;
    if let Some((socket, code, reason)) = ended {
        close(socket, code, reason).await;
    }
}

/// The viewer is gone: when it held the screen, Take over ends (`socket_closed`) and the held turns may start.
async fn leave(state: &Arc<State>, user: &User, id: &ConversationId, viewer: ViewerId) {
    let held = SCREENS
        .with(id, |screen| screen.close(viewer, Instant::now()))
        .flatten();
    if let Some(held) = held {
        handed_back(state, user, id, held, ScreenEnded::SocketClosed).await;
    }
}

async fn handed_back(
    state: &Arc<State>,
    user: &User,
    id: &ConversationId,
    held: Duration,
    ended: ScreenEnded,
) {
    let notice = Notice::HandedBack {
        seconds: held.as_secs(),
        ended,
    };
    if let Err(error) = record(state, id, user, notice).await {
        tracing::warn!(code = error.code(), "a hand-back's notice wasn't recorded");
    }
    if let Err(error) = state.manager.schedule(id) {
        tracing::warn!(
            code = error.code(),
            "the held turns weren't woken after a hand-back"
        );
    }
}

/// How long after a Take over's press control is granted at the latest: marked late when the cancelled run's end reset
/// isn't confirmed by then. Counted from the press, so a slow database or cancel can't push `you` past it.
pub(super) const RESET_WAIT: Duration = Duration::from_secs(10);

/// Takes over for `viewer`: the active run is cancelled as Stop does, and the notice recorded. Control is granted once
/// the runtime's run-end reset is over (`screen.reset`), so it can't clear a sign-in made under control: the cancelled
/// run's, or that of a run that had just ended. At once when there is none to wait for (GW-U-155, GW-I-230). All of
/// it runs off the relay, so the picture keeps coming meanwhile.
fn taken_over(state: &Arc<State>, user: &User, id: &ConversationId, viewer: ViewerId) {
    let deadline = tokio::time::Instant::now() + RESET_WAIT;
    // The resets so far, noted before the run is read: one that comes meanwhile still counts as the cancelled run's.
    let resets = SCREENS.with(id, |screen| {
        let resets = screen.resets();
        let before = resets.borrow().done;
        (resets, before)
    });
    let (sent, cancelled) = tokio::sync::oneshot::channel();
    let (state, user, id) = (state.clone(), user.clone(), id.clone());
    let conversation = id.clone();
    // The cancel and the notice always finish, even past the deadline.
    tokio::spawn(async move {
        let _ = sent.send(cancel_run(&state, &id).await);
        if let Err(error) = record(&state, &id, &user, Notice::TakenOver).await {
            tracing::warn!(code = error.code(), "a Take over's notice wasn't recorded");
        }
    });
    tokio::spawn(async move {
        let over = async {
            let cancelled = cancelled.await.unwrap_or(false);
            let Some((resets, before)) = resets else {
                return true;
            };
            super::viewers::reset_over(resets, before, cancelled).await
        };
        let ready = matches!(tokio::time::timeout_at(deadline, over).await, Ok(true));
        if !ready {
            tracing::info!("Take over granted before the run's end reset was confirmed");
        }
        SCREENS.grant(&conversation, viewer, ready);
    });
}

/// Cancels the task's active run, as Stop does: whether a cancel reached the runtime.
async fn cancel_run(state: &State, id: &ConversationId) -> bool {
    let run = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        crate::teams::store::current_run(&mut connection, crate::conversations::uuid(id)?).await
    })
    .await;
    let Ok(Some(run)) = run else {
        return false;
    };
    let Ok(run_id) = run.parse::<tqa_types::RunId>() else {
        return false;
    };
    match state
        .links
        .deliver(id, ClientFrame::RunCancel { run_id })
        .await
    {
        Ok(()) => true,
        Err(error) => {
            tracing::debug!(code = error.code(), "Take over's cancel was not delivered");
            false
        }
    }
}

/// Why a viewer lost its picture: the sandbox isn't running (the task has none; the panel waits for the next run), or
/// the screen isn't available just now (a bridge dropped or never came while the sandbox lives, its link up or
/// redialling; the panel retries). A store that can't say gets the retried one.
async fn unavailable(state: &State, id: &ConversationId) -> &'static str {
    if matches!(running(state, id).await, Ok(false)) {
        "sandbox_not_running"
    } else {
        "screen_unavailable"
    }
}

/// The gateway's text frame telling this viewer who holds the screen.
/// With `you` only, and never both: `ready` false when control was granted before the run's end reset was confirmed (a
/// sign-in made now may not last, GW-U-155); `reset_ok` false when the reset couldn't empty the browser's profile
/// (earlier sign-ins may still be there, GW-U-156). Live only, never stored.
fn control(holder: Holder, viewer: ViewerId) -> Message {
    let (holder, extra) = match holder {
        Holder::Nobody => ("none", ""),
        Holder::Viewer(holding) if holding == viewer => ("you", ""),
        Holder::Late(holding) if holding == viewer => ("you", r#","ready":false"#),
        Holder::Unclean(holding) if holding == viewer => ("you", r#","reset_ok":false"#),
        Holder::Viewer(_) | Holder::Late(_) | Holder::Unclean(_) => ("other", ""),
    };
    Message::Text(format!(r#"{{"type":"control","holder":"{holder}"{extra}}}"#).into())
}

/// What came of a viewer's text frame.
enum Command {
    Done,
    /// Neither Take over nor a hand-back: the viewer is refused.
    Invalid,
    /// The viewer's socket is gone.
    Gone,
}

/// A viewer's text frame: Take over, or a hand-back.
async fn command(
    state: &Arc<State>,
    user: &User,
    id: &ConversationId,
    viewer: ViewerId,
    (socket, text): (&mut WebSocket, &str),
) -> Command {
    match text {
        r#"{"type":"take_over"}"# => {
            match SCREENS.with(id, |screen| screen.take_over(viewer, Instant::now())) {
                Some(Ok(true)) => taken_over(state, user, id, viewer),
                // Another tab holds it (granted, or still waiting for the reset): this one keeps its picture and is told.
                Some(Err(_)) => {
                    let other = Message::Text(r#"{"type":"control","holder":"other"}"#.into());
                    if socket.send(other).await.is_err() {
                        return Command::Gone;
                    }
                }
                Some(Ok(false)) | None => {}
            }
            Command::Done
        }
        r#"{"type":"hand_back"}"# => {
            let held = SCREENS
                .with(id, |screen| screen.hand_back(viewer, Instant::now()))
                .flatten();
            if let Some(held) = held {
                handed_back(state, user, id, held, ScreenEnded::HandBack).await;
            }
            Command::Done
        }
        _ => Command::Invalid,
    }
}

/// Relays until either side ends. A refusal of the viewer comes back with its socket, to close with its code.
async fn relay(
    state: &Arc<State>,
    user: &User,
    id: &ConversationId,
    (mut socket, mut bridge): (WebSocket, WebSocket),
    (viewer, mut holder): (ViewerId, tokio::sync::watch::Receiver<Holder>),
) -> Option<(WebSocket, u16, &'static str)> {
    let mut filter = ClientFilter::new();
    // The holder is copied out first: the watch's read guard must not live across an await.
    let now = *holder.borrow_and_update();
    // The last control frame this viewer was sent: a change that says nothing new to it (another viewer's late grant
    // settled, the CU review's #20) isn't sent again.
    let mut sent = control(now, viewer);
    if socket.send(sent.clone()).await.is_err() {
        return None;
    }
    let ended = loop {
        let due = filter.next_due();
        tokio::select! {
            sent = socket.recv() => match sent {
                Some(Ok(Message::Binary(bytes))) => {
                    let holds = SCREENS.with(id, |screen| screen.holds(viewer)).unwrap_or(false);
                    let Ok(forward) = filter.feed(&bytes, holds, Instant::now()) else {
                        break Some((socket, 1008, "invalid_frame"));
                    };
                    if !forward.is_empty() && bridge.send(Message::Binary(forward.into())).await.is_err() {
                        break Some((socket, 4409, unavailable(state, id).await));
                    }
                }
                Some(Ok(Message::Text(text))) => match command(state, user, id, viewer, (&mut socket, &text)).await {
                    Command::Done => {}
                    Command::Invalid => break Some((socket, 1008, "invalid_frame")),
                    Command::Gone => break None,
                },
                Some(Ok(Message::Close(_)) | Err(_)) | None => break None,
                Some(Ok(_)) => {}
            },
            from = bridge.recv() => match from {
                Some(Ok(Message::Binary(bytes))) => {
                    if socket.send(Message::Binary(bytes)).await.is_err() {
                        break None;
                    }
                }
                Some(Ok(Message::Close(_)) | Err(_)) | None => break Some((socket, 4409, unavailable(state, id).await)),
                Some(Ok(_)) => {}
            },
            changed = holder.changed() => {
                if changed.is_err() {
                    break None;
                }
                let now = *holder.borrow_and_update();
                // The screen left this viewer: whatever it still had pressed is let go first.
                if !matches!(now, Holder::Viewer(holding) | Holder::Late(holding) | Holder::Unclean(holding) if holding == viewer)
                    && let Some(release) = filter.release()
                    && bridge.send(Message::Binary(release.into())).await.is_err()
                {
                    break Some((socket, 4409, unavailable(state, id).await));
                }
                let next = control(now, viewer);
                if next != sent {
                    if socket.send(next.clone()).await.is_err() {
                        break None;
                    }
                    sent = next;
                }
            }
            () = sleep_until(due), if due.is_some() => {
                if let Some(request) = filter.due(Instant::now())
                    && bridge.send(Message::Binary(request.into())).await.is_err()
                {
                    break Some((socket, 4409, unavailable(state, id).await));
                }
            }
        }
    };
    // However the relay ended, nothing this viewer pressed stays down on the desktop.
    if let Some(release) = filter.release() {
        let _ = bridge.send(Message::Binary(release.into())).await;
    }
    ended
}

async fn sleep_until(due: Option<Instant>) {
    if let Some(due) = due {
        tokio::time::sleep_until(due.into()).await;
    }
}
