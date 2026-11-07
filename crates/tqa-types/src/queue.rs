//! Bounded admission metadata for transient session-ready notifications.
use crate::{ErrorCode, FrameError};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Largest number of waiting runs that a queue notification can report ahead of this run.
pub const MAX_QUEUE_AHEAD: u32 = 10_000;

/// What a queued run waits for.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueueReason {
    /// A free sandbox slot on the host (admission).
    Slot,
    /// The workspace, whose write lease another task's sandbox holds.
    Workspace,
    /// Nothing but the same person's own idle sandbox, retiring to make room (revision 10, F26): from their side the
    /// task is starting, not queued behind anyone.
    OwnSandbox,
    /// A free Desktop: as many desktop sandboxes as `desktop.max_sessions` allows are live (revision 14, ADR-0039).
    DesktopSessions,
}

/// A validated queue position. Future metadata fields are ignored by receivers.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct QueuePosition {
    /// Waiting runs served before this one; zero means next.
    #[schemars(range(max = 10000))]
    ahead: u32,
    /// What the run waits for; absent from older gateways. A reason this version does not know is ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<QueueReason>,
}

impl QueuePosition {
    /// Records the number of waiting runs served before this one, in `0..=10000`.
    ///
    /// # Errors
    /// Returns `invalid_frame` if the position exceeds the protocol's queue bound.
    pub fn new(ahead: u32) -> Result<Self, FrameError> {
        if ahead > MAX_QUEUE_AHEAD {
            return Err(FrameError {
                code: ErrorCode::InvalidFrame,
                path: "queue.ahead".into(),
                detail: "ahead must be at most 10000".into(),
            });
        }
        Ok(Self {
            ahead,
            reason: None,
        })
    }

    /// The same position, saying what the run waits for.
    #[must_use]
    pub const fn with_reason(self, reason: QueueReason) -> Self {
        Self {
            ahead: self.ahead,
            reason: Some(reason),
        }
    }

    /// What the run waits for, when the sender said.
    #[must_use]
    pub const fn reason(self) -> Option<QueueReason> {
        self.reason
    }

    /// Returns the number of waiting runs served before this one; zero means next.
    #[must_use]
    pub const fn ahead(self) -> u32 {
        self.ahead
    }
}

#[derive(Deserialize)]
struct QueueFields {
    ahead: u32,
    // A string, so a reason added by a later version is ignored instead of failing the frame.
    #[serde(default)]
    reason: Option<String>,
}

impl<'de> Deserialize<'de> for QueuePosition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let fields = QueueFields::deserialize(deserializer)?;
        let position = Self::new(fields.ahead).map_err(D::Error::custom)?;
        Ok(match fields.reason.as_deref() {
            Some("slot") => position.with_reason(QueueReason::Slot),
            Some("workspace") => position.with_reason(QueueReason::Workspace),
            Some("own_sandbox") => position.with_reason(QueueReason::OwnSandbox),
            Some("desktop_sessions") => position.with_reason(QueueReason::DesktopSessions),
            _ => position,
        })
    }
}
