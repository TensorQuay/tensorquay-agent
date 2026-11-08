//! Forward-compatible server frames shared by runtime links and client streams.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ConversationId, ErrorCode, Event, FrameError, RunId, SandboxState, Seq};

/// The sandbox execution backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SandboxBackend {
    /// A container sandbox.
    Docker,
    /// A microVM sandbox.
    Firecracker,
}

/// The gateway's prepared-action risk classification.
/// Public sandbox status; future summary fields are ignored by receivers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxSummary {
    /// The sandbox execution backend.
    pub backend: SandboxBackend,
    /// The current lifecycle state.
    pub state: SandboxState,
    /// Present while the gateway replaces a sandbox that stopped unexpectedly, and when it gave up doing so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<SandboxRecovery>,
}

impl SandboxSummary {
    /// A summary with no recovery in progress.
    #[must_use]
    pub const fn new(backend: SandboxBackend, state: SandboxState) -> Self {
        Self {
            backend,
            state,
            recovery: None,
        }
    }
}

/// The replacement of a sandbox that stopped unexpectedly: after a few losses in a short window the gateway waits
/// longer before each replacement, and past its limit it stops (`failed`) until the owner acts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxRecovery {
    /// How many times this conversation's sandbox stopped unexpectedly in the last 10 minutes, this one included.
    #[schemars(range(min = 1))]
    pub losses: u32,
    /// Seconds until the gateway starts the replacement; absent when it will not replace it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_in_secs: Option<u32>,
}

/// Server frames ignore unknown fields so additive changes preserve existing consumers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ServerFrame {
    /// Supervisor liveness on the runtime-to-gateway link only; never persisted or sent to clients.
    #[serde(rename = "heartbeat")]
    Heartbeat {
        /// The active run whose supervisor is alive; this frame has no sequence.
        run_id: RunId,
    },
    /// Transient sandbox status, sent before replay and on changes; never sequenced or stored.
    #[serde(rename = "session.ready")]
    SessionReady {
        /// The conversation whose sandbox status is being reported.
        conversation_id: ConversationId,
        /// Whether an existing conversation was resumed.
        resumed: bool,
        /// The sandbox backend and current state.
        sandbox: SandboxSummary,
        /// The gateway's queue position, sent only while the sandbox state is queued.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        queue: Option<crate::QueuePosition>,
    },
    /// An event carries its durable sequence and run context.
    #[serde(rename = "event")]
    Event {
        /// The per-conversation sequence number.
        seq: Seq,
        /// The associated run, when the event belongs to one.
        run_id: Option<RunId>,
        /// The associated step, when applicable.
        step: Option<u32>,
        /// The RFC 3339 event timestamp.
        #[serde(deserialize_with = "crate::validation::timestamp")]
        #[schemars(schema_with = "crate::validation::timestamp_schema")]
        ts: String,
        /// The typed lifecycle or streaming event.
        event: Event,
    },
    /// Retired by ADR-0035 (an approval request): read from histories written before it, never sent. It parses the
    /// old stored payload and keeps only the sequence, which the store checks; the web ignores it on replay.
    #[serde(rename = "permission.request")]
    PermissionRequest {
        /// The per-conversation sequence number.
        seq: Seq,
    },
    /// A classified failure, optionally tied to a sequence.
    #[serde(rename = "error")]
    Error {
        /// The associated sequence when one is available.
        seq: Option<Seq>,
        /// The stable error classification.
        code: ErrorCode,
        /// A human-readable explanation.
        message: String,
        /// Whether this particular failure can be retried.
        retryable: bool,
        /// The bounded network destination when this failure concerns egress.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<crate::ErrorTarget>,
    },
    /// Runtime-to-gateway workspace reply part: unsequenced, strict, never stored or forwarded to clients.
    #[serde(rename = "workspace.reply")]
    WorkspaceReply(crate::WorkspaceReply),
    /// Gateway to clients only: the task's messages waiting behind its run changed ("send after this"). Unsequenced
    /// and never stored; the list endpoint is the durable view. A runtime never sends it.
    #[serde(rename = "message.queue")]
    MessageQueue {
        /// Messages waiting for the run to end, oldest first.
        held: Vec<String>,
        /// Messages dropped because the run failed or was stopped.
        dropped: Vec<String>,
        /// When a turn went out: the messages joined into it, in order.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sent: Option<Vec<String>>,
    },
    /// Gateway to clients only: the model named the task (its short name after the first completed run, L7); a
    /// person's rename is not announced here, so a receiver may treat the title as the model's. Unsequenced and never
    /// stored; the conversation record is the durable view. A runtime never sends it.
    #[serde(rename = "task.titled")]
    TaskTitled {
        /// The new title: 1 to 200 characters on one line, not blank, without control characters.
        #[schemars(length(min = 1, max = 200))]
        title: String,
    },
    /// Gateway to clients only (ADR-0034, the slice S6 contract): the task's agent team changed, on the lead's stream
    /// and each member's. Unsequenced, never stored and never in a replay, so it never moves a client's cursor; at most
    /// one a second per team, the latest winning. The team's routes are the durable view: a client reads the team when
    /// `version` rose and its messages after its last seq when `last_seq` did. A runtime never sends or receives it, so
    /// it takes no revision row (ADR-0026).
    #[serde(rename = "team.changed")]
    TeamChanged {
        /// The agent team.
        #[schemars(regex(
            pattern = r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
        ))]
        team: String,
        /// The team's version: it rises with every change to the team or a helper's state.
        version: u64,
        /// The seq of the team's latest message.
        last_seq: u64,
    },
    /// Gateway to clients only (ADR-0039's contract): a screen notice, sent once when the gateway stores it. Unsequenced
    /// and never in a replay; `after_seq` places it in the transcript after that event, and the task's notices route
    /// is the durable view. A runtime never sends or receives it, so it takes no revision row (ADR-0026).
    #[serde(rename = "screen.notice")]
    ScreenNotice {
        /// What happened.
        kind: crate::ScreenNoticeKind,
        /// Who took over, handed back or signed out.
        by: crate::ScreenPerson,
        /// When, RFC 3339.
        #[serde(deserialize_with = "crate::validation::notice_at")]
        at: String,
        /// The conversation's last stored event seq when it happened.
        after_seq: u64,
        /// `screen.handed_back` only: how long Take over lasted, in seconds; absent when it ended by a `restart`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seconds: Option<u64>,
        /// `screen.handed_back` only: Hand back, the holder's socket closed, or the gateway restarted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ended: Option<crate::ScreenEnded>,
        /// `screen.sign_out_failed` only: why it couldn't be confirmed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<crate::ScreenNoticeReason>,
    },
    /// Runtime to gateway only (revision 14, ADR-0039 section 9): one chunk of a screenshot for the gateway's store.
    /// Never forwarded to clients or journaled; the gateway answers the last chunk with `screen.stored`.
    #[serde(rename = "screen.upload")]
    ScreenUpload {
        /// The PNG's SHA-256 in lowercase hex: its store id, as `screen.capture` names it.
        #[serde(deserialize_with = "crate::screen::sha256_hex")]
        #[schemars(regex(pattern = r"^[0-9a-f]{64}$"))]
        id: String,
        /// The chunk's place, counted from 0.
        chunk: u32,
        /// Whether this is the upload's final chunk.
        last: bool,
        /// Base64 of at most 96 KiB.
        #[schemars(length(min = 1, max = 131_072))]
        data: String,
    },
    /// Runtime to gateway only (revision 14, ADR-0039 section 6): its answer to `screen.sign_out`, after emptying the
    /// browser's profile or failing to, and after starting the browser again. The gateway records the notice from it
    /// and never forwards it to clients.
    #[serde(rename = "screen.sign_out.done")]
    ScreenSignOutDone {
        /// Whether the profile is empty now.
        ok: bool,
        /// Why not, when `ok` is false; absent otherwise.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<crate::SignOutFailure>,
        /// Whether the browser started again and its window took the focus, as `screen.reset`'s (`false`: a Take over
        /// after it says a sign-in now may not last). Absent from a runtime
        /// that doesn't say: true.
        #[serde(
            default = "crate::server::yes",
            skip_serializing_if = "crate::server::is_yes"
        )]
        ready: bool,
    },
    /// Runtime to gateway only (revision 14, ADR-0039 section 6): a desktop run's end reset is over, its browser's
    /// profile emptied (or not, `ok` false) and the browser started again. A Take over that cancelled the run is
    /// granted only after it, so the reset can't clear a sign-in made under control. Unsequenced; never forwarded.
    #[serde(rename = "screen.reset")]
    ScreenReset {
        /// Whether the profile was emptied.
        ok: bool,
        /// Whether the browser started again and its window took the focus, so a person's keys reach it (`false`: a
        /// Take over's control says a sign-in now may not last; the 16r int's `sec_059_a`). Absent from a runtime
        /// that doesn't say: true.
        #[serde(
            default = "crate::server::yes",
            skip_serializing_if = "crate::server::is_yes"
        )]
        ready: bool,
    },
    /// Gateway to clients only (revision 11, ADR-0037 §1.3 of the gateway plan): the run called a keyed host with its
    /// key, once per run and key. Unsequenced and never stored; the task's host-key uses are the durable view.
    #[serde(rename = "host_key.used")]
    HostKeyUsed {
        /// The run that made the call.
        run_id: RunId,
        /// The step running when the proxy saw the call; null when none was (always written).
        #[serde(default)]
        step: Option<u32>,
        /// The run's sender, whose keys the run holds.
        #[schemars(regex(
            pattern = r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
        ))]
        sender_id: String,
        /// The key's variable, only on the sender's own streams; null for every other viewer.
        #[schemars(regex(pattern = "^[A-Z][A-Z0-9_]{0,63}$"))]
        name: Option<String>,
        /// Where the call went: the CONNECT's host, a name or an IP address in canonical form (ADR-0037, Amendment 2).
        #[schemars(regex(pattern = crate::host_key::HOST_RULE), length(min = 1, max = 253))]
        host: String,
        /// And its port.
        #[schemars(range(min = 1))]
        port: u16,
    },
    /// Gateway to clients only (revision 11): the proxy refused a keyed call and nothing reached the host; at most once
    /// a minute per run, key and outcome. Unsequenced and never stored.
    #[serde(rename = "host_key.refused")]
    HostKeyRefused {
        /// The run that made the call.
        run_id: RunId,
        /// The step running when the proxy saw the call; null when none was (always written).
        #[serde(default)]
        step: Option<u32>,
        /// The run's sender.
        #[schemars(regex(
            pattern = r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
        ))]
        sender_id: String,
        /// The key's variable, only on the sender's own streams; null for every other viewer, and for `absent`.
        #[schemars(regex(pattern = "^[A-Z][A-Z0-9_]{0,63}$"))]
        name: Option<String>,
        /// Where the call went: the CONNECT's host, a name or an IP address in canonical form (ADR-0037, Amendment 2).
        #[schemars(regex(pattern = crate::host_key::HOST_RULE), length(min = 1, max = 253))]
        host: String,
        /// And its port.
        #[schemars(range(min = 1))]
        port: u16,
        /// Why it was refused.
        outcome: crate::HostKeyOutcome,
    },
}

/// Whether `id` is a team id as the gateway writes it: a lower-case hyphenated UUID.
pub(crate) fn is_team_id(id: &str) -> bool {
    id.len() == 36
        && id.char_indices().all(|(at, c)| match at {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        })
}

/// A task title's bound, as a conversation title's.
const MAX_TASK_TITLE_CHARS: usize = 200;

impl ServerFrame {
    /// The frame as clients receive it: a `reasoning.state` event keeps its size but never its
    /// opaque item (ADR-0027). Only the runtime's own history carries items.
    #[must_use]
    pub fn for_clients(mut self) -> Self {
        if let Self::Event {
            event: Event::ReasoningState { item, .. },
            ..
        } = &mut self
        {
            *item = None;
        }
        self
    }

    /// Parses a bounded server frame, ignoring additional fields but validating known values; a workspace reply is
    /// strict.
    ///
    /// # Errors
    /// Rejects oversized, malformed or unknown frames and invalid known fields.
    pub fn parse(bytes: &[u8]) -> Result<Self, FrameError> {
        let frame: Self = crate::parse::parse(
            bytes,
            &[
                "session.ready",
                "event",
                "permission.request",
                "error",
                "heartbeat",
                "workspace.reply",
                "message.queue",
                "task.titled",
                "team.changed",
                "screen.notice",
                "screen.sign_out.done",
                "screen.reset",
                "screen.upload",
                "host_key.used",
                "host_key.refused",
            ],
        )?;
        if let Self::HostKeyUsed {
            sender_id,
            name,
            host,
            port,
            ..
        }
        | Self::HostKeyRefused {
            sender_id,
            name,
            host,
            port,
            ..
        } = &frame
            && !crate::host_key::valid_notice(name.as_deref(), host, *port, sender_id)
        {
            return Err(FrameError {
                code: ErrorCode::InvalidFrame,
                path: "$".into(),
                detail: "expected a key's variable or null, a lower-case host, a port and the sender's id".into(),
            });
        }
        if let Self::WorkspaceReply(reply) = &frame {
            reply.check()?;
        }
        crate::screen_notice::check_frame(&frame)?;
        if let Self::TeamChanged { team, .. } = &frame
            && !is_team_id(team)
        {
            return Err(FrameError {
                code: ErrorCode::InvalidFrame,
                path: "team".into(),
                detail: "expected the team's id, a lower-case UUID".into(),
            });
        }
        if let Self::TaskTitled { title } = &frame {
            let characters = title.chars().count();
            if !(1..=MAX_TASK_TITLE_CHARS).contains(&characters)
                || title.trim().is_empty()
                || title.chars().any(char::is_control)
            {
                return Err(FrameError {
                    code: ErrorCode::InvalidFrame,
                    path: "title".into(),
                    detail: "expected 1 to 200 characters on one line without control characters"
                        .into(),
                });
            }
        }
        Ok(frame)
    }
}

/// A field that is true when absent.
pub(crate) const fn yes() -> bool {
    true
}

/// Whether a field defaulting to [`yes`] is true, so it is left out on the wire as before it existed.
#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes the field by reference
pub(crate) const fn is_yes(value: &bool) -> bool {
    *value
}
