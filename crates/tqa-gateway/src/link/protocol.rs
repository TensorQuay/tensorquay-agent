use crate::{GatewayError, wire};
use async_trait::async_trait;
use tqa_types::{ConversationId, RunId, SandboxState, ServerFrame};

/// How long the link waits on the journal for one runtime frame. A store that is slow, as under host I/O pressure
/// with commits of several seconds, keeps its links; one that stays unavailable still closes them, and the runtime
/// resends from its write-ahead log when it redials (GW-E-020).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StoreBounds {
    /// One append attempt: the gateway's `io_timeout`, which also bounds the attempt's statements.
    pub attempt: std::time::Duration,
    /// Every attempt for the frame together, backoff included: three attempts' worth. It stays well inside the
    /// link's own liveness (a pong within 90 s), and the runtime's writes meanwhile go to the socket's buffers.
    pub budget: std::time::Duration,
}

impl StoreBounds {
    /// The bounds for a gateway whose I/O timeout is `io_timeout`.
    pub(crate) const fn new(io_timeout: std::time::Duration) -> Self {
        Self {
            attempt: io_timeout,
            budget: io_timeout.saturating_mul(3),
        }
    }
}

/// The runtime receiver needs durable append and ephemeral publication, not database access.
#[async_trait]
pub(crate) trait FrameSink: Send + Sync {
    async fn append(
        &self,
        conversation: &ConversationId,
        frame: ServerFrame,
    ) -> Result<(), GatewayError>;
    async fn recover_committed(
        &self,
        _conversation: &ConversationId,
        _frame: &ServerFrame,
    ) -> Result<bool, GatewayError> {
        Ok(false)
    }
    /// Journals consecutive plain journal frames together. A journal overrides this with one atomic transaction
    /// (the gateway's `EventStore`); this default, for sinks that store nothing durable, takes them one by one.
    async fn append_batch(
        &self,
        conversation: &ConversationId,
        frames: &[ServerFrame],
    ) -> Result<(), GatewayError> {
        for frame in frames {
            self.append(conversation, frame.clone()).await?;
        }
        Ok(())
    }
    /// After an uncertain batch: whether every frame of it is stored as sent, then published. A journal overrides
    /// this to check them all before publishing any; this default checks them one by one.
    async fn recover_committed_batch(
        &self,
        conversation: &ConversationId,
        frames: &[ServerFrame],
    ) -> Result<bool, GatewayError> {
        for frame in frames {
            if !self.recover_committed(conversation, frame).await? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn control(
        &self,
        conversation: &ConversationId,
        frame: ServerFrame,
    ) -> Result<(), GatewayError>;
}

/// What the link must do after a runtime frame was accepted.
#[derive(Debug, PartialEq)]
pub(crate) enum Received {
    /// The frame was journaled or published; nothing else follows.
    Accepted,
    /// A supervisor heartbeat: liveness only, never stored.
    Heartbeat(RunId),
    /// A journaled run lifecycle event moved the client-facing sandbox state; `completed` for a `run.completed`.
    Transition {
        sandbox: SandboxState,
        completed: bool,
    },
    /// A part of a workspace reply (ADR-0030): it belongs to the gateway's request, never to clients or the journal.
    Workspace(Box<tqa_types::WorkspaceReply>),
    /// A chunk of a screenshot for the store (ADR-0039 section 9): never journaled, answered on this link.
    ScreenUpload {
        id: String,
        chunk: u32,
        last: bool,
        data: String,
    },
    /// The runtime refused a message as busy: the turn of held messages goes back to waiting when it was one
    /// (GW-I-129); otherwise the refusal reaches clients as any other.
    Busy(Box<ServerFrame>),
}

/// Why a frame a runtime sent is refused outright: the gateway's own notices to clients, the proxy's notices of keyed
/// calls (ADR-0037), and the retired approval and park frames (ADR-0035), read from stored histories only.
fn not_from_a_runtime(frame: &ServerFrame) -> Option<&'static str> {
    match frame {
        ServerFrame::MessageQueue { .. } => Some("a runtime may not send message.queue"),
        ServerFrame::TaskTitled { .. } => Some("a runtime may not send task.titled"),
        ServerFrame::TeamChanged { .. } => Some("a runtime may not send team.changed"),
        ServerFrame::ScreenNotice { .. } => Some("a runtime may not send screen.notice"),
        ServerFrame::HostKeyUsed { .. } | ServerFrame::HostKeyRefused { .. } => {
            Some("a runtime may not send host_key.used or host_key.refused")
        }
        frame if retired(frame) => {
            Some("a runtime may not send a retired approval or park frame (ADR-0035)")
        }
        _ => None,
    }
}

pub(crate) async fn receive(
    store: &dyn FrameSink,
    conversation: &ConversationId,
    bytes: &[u8],
    bounds: StoreBounds,
) -> Result<Received, GatewayError> {
    let frame = ServerFrame::parse(bytes).map_err(|error| GatewayError::Protocol(error.code))?;
    if let Some(why) = not_from_a_runtime(&frame) {
        return Err(GatewayError::Forbidden(why));
    }
    let frame = match frame {
        ServerFrame::Heartbeat { run_id } => return Ok(Received::Heartbeat(run_id)),
        ServerFrame::WorkspaceReply(reply) => return Ok(Received::Workspace(Box::new(reply))),
        ServerFrame::ScreenUpload {
            id,
            chunk,
            last,
            data,
        } => {
            return Ok(Received::ScreenUpload {
                id,
                chunk,
                last,
                data,
            });
        }
        // Sign out everywhere's answer (ADR-0039): to the sign-out waiting for it, never to clients or the journal.
        ServerFrame::ScreenSignOutDone { ok, reason, ready } => {
            crate::screen::answered(conversation, ok, reason, ready);
            return Ok(Received::Accepted);
        }
        // A desktop run's end reset is over (ADR-0039 section 6): a Take over waiting for it gets control.
        ServerFrame::ScreenReset { ok, ready } => {
            if !ok {
                tracing::warn!(
                    "the runtime couldn't empty the desktop browser's profile after a run"
                );
            }
            crate::screen::reset_done(conversation, ok, ready);
            return Ok(Received::Accepted);
        }
        frame @ ServerFrame::Error {
            code: tqa_types::ErrorCode::Busy,
            seq: None,
            ..
        } => return Ok(Received::Busy(Box::new(frame))),
        frame => frame,
    };
    if let ServerFrame::SessionReady {
        conversation_id, ..
    } = &frame
        && conversation_id != conversation
    {
        return Err(GatewayError::Forbidden(
            "runtime conversation binding differs",
        ));
    }
    if wire::sequence(&frame).is_some() {
        let transition = wire::run_transition(&frame);
        let completed = matches!(
            &frame,
            ServerFrame::Event {
                event: tqa_types::Event::RunCompleted { .. },
                ..
            }
        );
        append(store, conversation, std::slice::from_ref(&frame), bounds).await?;
        if matches!(
            &frame,
            ServerFrame::Event {
                event: tqa_types::Event::RunCompleted { .. }
                    | tqa_types::Event::RunFailed { .. }
                    | tqa_types::Event::RunCancelled { .. },
                ..
            }
        ) {
            crate::screen::run_ended(conversation);
        }
        if let Some(sandbox) = transition {
            return Ok(Received::Transition { sandbox, completed });
        }
    } else {
        store.control(conversation, frame)?;
    }
    Ok(Received::Accepted)
}

/// A plain journal frame: a sequenced event with no effect of its own beyond being stored and published, so it may
/// be journaled in a batch (GW-U-116). A run lifecycle event moves the sandbox state and has per-event effects in its
/// transaction, so it is never one; nor is a frame that doesn't parse, which is received alone to report why.
pub(crate) fn plain_journal(bytes: &[u8]) -> Option<ServerFrame> {
    let frame = ServerFrame::parse(bytes).ok()?;
    let plain = matches!(frame, ServerFrame::Event { .. })
        && wire::sequence(&frame).is_some()
        && wire::run_transition(&frame).is_none()
        && !retired(&frame);
    plain.then_some(frame)
}

/// Journals a batch of plain journal frames (GW-U-116, GW-U-117).
pub(crate) async fn receive_batch(
    store: &dyn FrameSink,
    conversation: &ConversationId,
    frames: Vec<ServerFrame>,
    bounds: StoreBounds,
) -> Result<Received, GatewayError> {
    append(store, conversation, &frames, bounds).await?;
    Ok(Received::Accepted)
}

/// The frames ADR-0035 retired: kept in the protocol so histories written before it still load, never sent again.
const fn retired(frame: &ServerFrame) -> bool {
    matches!(
        frame,
        ServerFrame::PermissionRequest { .. }
            | ServerFrame::Error {
                code: tqa_types::ErrorCode::PermissionBinding,
                ..
            }
            | ServerFrame::Event {
                event: tqa_types::Event::PermissionResolved { .. }
                    | tqa_types::Event::PermissionExpired { .. }
                    | tqa_types::Event::RunParked { .. },
                ..
            }
    )
}

/// How an attempt's frames came to be stored.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Took {
    /// This attempt journaled them.
    Appended,
    /// An earlier attempt whose answer was lost had journaled them; they were found stored as sent.
    Recovered,
}

/// Journals `frames` in order. Each part gets attempts bounded by `bounds.attempt` and a budget of `bounds.budget`
/// that starts again once a part is stored. After an attempt with no answer, the part is halved (GW-U-117), down to
/// one frame, and the next attempt is uncertain: a refusal as already stored then recovers the frames if they match.
async fn append(
    store: &dyn FrameSink,
    conversation: &ConversationId,
    frames: &[ServerFrame],
    bounds: StoreBounds,
) -> Result<(), GatewayError> {
    let mut cursor = Cursor {
        done: 0,
        size: frames.len(),
        uncertain: false,
        attempts: 0,
    };
    while cursor.done < frames.len() {
        cursor.attempts = 0;
        let part = store_part(store, conversation, frames, &mut cursor, bounds);
        let Ok(stored) = tokio::time::timeout(bounds.budget, part).await else {
            tracing::warn!(
                operation = "journal_append",
                attempts = cursor.attempts,
                frames = frames.len() - cursor.done,
                budget_ms = u64::try_from(bounds.budget.as_millis()).unwrap_or(u64::MAX),
                "the journal did not take a runtime frame within the budget; closing the link"
            );
            return Err(GatewayError::StoreUnavailable);
        };
        let (count, took) = stored?;
        cursor.done += count;
        // A fresh append settles every earlier uncertain one; a recovered part may be followed by more of it.
        if took == Took::Appended {
            cursor.uncertain = false;
        }
    }
    Ok(())
}

/// Where a batch's append stands: frames stored, the next part's size, whether the next attempt is uncertain, and
/// the attempts at the current part.
struct Cursor {
    done: usize,
    size: usize,
    uncertain: bool,
    attempts: u32,
}

/// Attempts the next part until it is stored or refused; after an attempt with no answer, the part is halved.
async fn store_part(
    store: &dyn FrameSink,
    conversation: &ConversationId,
    frames: &[ServerFrame],
    cursor: &mut Cursor,
    bounds: StoreBounds,
) -> Result<(usize, Took), GatewayError> {
    let mut backoff = std::time::Duration::from_millis(50);
    loop {
        cursor.attempts += 1;
        let part = &frames[cursor.done..frames.len().min(cursor.done + cursor.size)];
        // An attempt past its bound may still commit: the next one is uncertain and recovers it.
        let result = tokio::time::timeout(
            bounds.attempt,
            attempt(store, conversation, part, cursor.uncertain),
        )
        .await
        .unwrap_or(Err(GatewayError::Timeout));
        if !matches!(
            result,
            Err(GatewayError::StoreUnavailable | GatewayError::Timeout)
        ) {
            return result.map(|took| (part.len(), took));
        }
        cursor.uncertain = true;
        cursor.size = (part.len() / 2).max(1);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(std::time::Duration::from_millis(500));
    }
}

async fn attempt(
    store: &dyn FrameSink,
    conversation: &ConversationId,
    part: &[ServerFrame],
    uncertain: bool,
) -> Result<Took, GatewayError> {
    let result = match part {
        [frame] => store.append(conversation, frame.clone()).await,
        part => store.append_batch(conversation, part).await,
    };
    if uncertain && result == Err(GatewayError::Protocol(tqa_types::ErrorCode::SeqGap)) {
        let recovered = match part {
            [frame] => store.recover_committed(conversation, frame).await?,
            part => store.recover_committed_batch(conversation, part).await?,
        };
        if recovered {
            return Ok(Took::Recovered);
        }
    }
    result.map(|()| Took::Appended)
}
