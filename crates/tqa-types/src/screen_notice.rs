//! The gateway's own screen notices (ADR-0039's contract): Take over, its hand-back and Sign out everywhere. They go to
//! clients only, as a stream frame and through the task's notices route; a runtime never sends or receives one. Also
//! Sign out everywhere's answer from the runtime (`screen.sign_out.done`), which the gateway turns into a notice.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ErrorCode, FrameError, ServerFrame};

/// What happened to the task's screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ScreenNoticeKind {
    /// A person took over the desktop; the run was cancelled as Stop does.
    #[serde(rename = "screen.taken_over")]
    TakenOver,
    /// Take over ended: Hand back, the holder's socket closed, or the gateway restarted.
    #[serde(rename = "screen.handed_back")]
    HandedBack,
    /// The desktop's browser was signed out everywhere: its profile emptied.
    #[serde(rename = "screen.signed_out")]
    SignedOut,
    /// Sign out everywhere couldn't be confirmed done: the runtime couldn't clear the profile, or never answered.
    #[serde(rename = "screen.sign_out_failed")]
    SignOutFailed,
}

/// Why Sign out everywhere couldn't be confirmed (`screen.sign_out_failed`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreenNoticeReason {
    /// The runtime answered that it couldn't empty the browser's profile.
    NotCleared,
    /// The runtime didn't answer within 10 s, or its link dropped first.
    Unconfirmed,
}

/// Why the runtime couldn't empty the browser's profile (`screen.sign_out.done` with `ok: false`); a closed list.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignOutFailure {
    /// The closing browser still held a file of the profile.
    ProfileInUse,
    /// Any other failure of the file system, a full tmpfs included.
    IoError,
}

/// How Take over ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreenEnded {
    /// The person pressed Hand back.
    HandBack,
    /// The holder's viewer socket closed, a dropped network included.
    SocketClosed,
    /// The gateway restarted while Take over was held: recorded at its next start, with no length (how long it was
    /// held before the restart isn't known).
    Restart,
}

/// The person a notice names.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenPerson {
    /// The person's id, a lower-case UUID.
    #[schemars(regex(
        pattern = r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
    ))]
    pub id: String,
    /// Their sign-in address, the gateway's name for a person: 1 to 254 bytes on one line.
    #[schemars(length(min = 1, max = 254))]
    pub name: String,
}

/// The longest name a notice carries, in bytes: a sign-in address's bound.
const MAX_NAME_BYTES: usize = 254;

/// Whether a notice's fields agree: a person's id and name as above, `ended` on a hand-back only and always there,
/// `seconds` on a hand-back only and always there but for one ended by a `restart`, and `reason` on a failed sign-out
/// only.
fn check(
    kind: ScreenNoticeKind,
    by: &ScreenPerson,
    (seconds, ended): (Option<u64>, Option<ScreenEnded>),
    reason: Option<ScreenNoticeReason>,
) -> Result<(), &'static str> {
    if !crate::server::is_team_id(&by.id) {
        return Err("by.id: expected the person's id, a lower-case UUID");
    }
    if !(1..=MAX_NAME_BYTES).contains(&by.name.len())
        || by.name.trim().is_empty()
        || by.name.chars().any(char::is_control)
    {
        return Err("by.name: expected 1 to 254 bytes on one line");
    }
    let hand_back = kind == ScreenNoticeKind::HandedBack;
    if hand_back != ended.is_some() {
        return Err("ended comes with screen.handed_back only, and always with it");
    }
    if (hand_back && ended != Some(ScreenEnded::Restart)) != seconds.is_some() {
        return Err("seconds comes with a hand-back's length only: never with ended restart");
    }
    if (kind == ScreenNoticeKind::SignOutFailed) != reason.is_some() {
        return Err("reason comes with screen.sign_out_failed only, and always with it");
    }
    Ok(())
}

/// A parsed frame's screen notice or sign-out answer, if it is one: a notice checked as [`check`] says, an answer with a
/// `reason` exactly when it isn't `ok`.
pub(crate) fn check_frame(frame: &ServerFrame) -> Result<(), FrameError> {
    let refused = match frame {
        ServerFrame::ScreenNotice {
            kind,
            by,
            seconds,
            ended,
            reason,
            ..
        } => check(*kind, by, (*seconds, *ended), *reason).err(),
        ServerFrame::ScreenSignOutDone { ok, reason, .. } => (*ok == reason.is_some())
            .then_some("reason comes with ok: false only, and always with it"),
        ServerFrame::ScreenUpload { data, .. } => (!crate::screen_upload::valid_chunk(data))
            .then_some("data: expected padded base64 of at most 96 KiB"),
        _ => None,
    };
    refused.map_or(Ok(()), |detail| {
        Err(FrameError {
            code: ErrorCode::InvalidFrame,
            path: "$".into(),
            detail: detail.into(),
        })
    })
}
