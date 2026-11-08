//! The shape of a task's place in an agent team and who sees what of it (ADR-0034, the slice S6 contract, SEC-005):
//! the task's own person gets every field, anyone else only the role, the team's id and state, a lead's counts and a
//! member's lead id. The 100 % class (ENGINEERING-GUARDRAILS §2.2, a permission rule): every branch unit-tested here;
//! the reads that fill it are `place`'s.
use serde::Serialize;
use uuid::Uuid;

/// A task's place in an agent team.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum TeamPlace {
    /// The task leads the team.
    Lead {
        /// The team.
        id: Uuid,
        /// `running`, `stopping` or `ended`.
        state: String,
        /// Its helpers now, deleted ones not counted.
        helpers: u32,
        /// How many of them are stuck or failed.
        attention: Attention,
        /// A `person_question` is unanswered (its own person only).
        #[serde(skip_serializing_if = "Option::is_none")]
        waiting_for_you: Option<bool>,
    },
    /// The task is a helper of the team's lead.
    Member {
        /// The team.
        id: Uuid,
        /// `running`, `stopping` or `ended`.
        state: String,
        /// The lead task.
        lead: Lead,
        /// The helper's name (its own person only).
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// The helper's state (its own person only).
        #[serde(skip_serializing_if = "Option::is_none")]
        helper_state: Option<&'static str>,
        /// Whether its person's Stop helper would stop it now (its own person only; [`can_stop`]).
        #[serde(skip_serializing_if = "Option::is_none")]
        can_stop: Option<bool>,
    },
}

/// A lead's helpers that need attention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Attention {
    /// Stuck helpers.
    pub stuck: u32,
    /// Failed helpers.
    pub failed: u32,
}

/// A member's lead task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Lead {
    /// The lead task's id.
    pub id: Uuid,
    /// Its title as its person sees it: present for its own person only, and null there when untitled.
    #[serde(flatten)]
    pub title: Option<Title>,
}

/// A lead task's title, for its own person (`null` when untitled).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Title {
    /// The title.
    pub title: Option<String>,
}

impl TeamPlace {
    /// The place as `viewer` may see it: the task's own person sees everything, anyone else only what isn't content.
    pub(super) fn for_viewer(self, own: bool) -> Self {
        if own {
            return self;
        }
        match self {
            Self::Lead {
                id,
                state,
                helpers,
                attention,
                ..
            } => Self::Lead {
                id,
                state,
                helpers,
                attention,
                waiting_for_you: None,
            },
            Self::Member {
                id, state, lead, ..
            } => Self::Member {
                id,
                state,
                lead: Lead {
                    id: lead.id,
                    title: None,
                },
                name: None,
                helper_state: None,
                can_stop: None,
            },
        }
    }
}

/// A helper's state as people read it: the stored one, with `starting` shown as `working` (its first turn is going
/// out) and `waiting` as `waiting_lead`.
pub(crate) fn helper_state(stored: &str) -> &'static str {
    match stored {
        "starting" | "working" => "working",
        "waiting" => "waiting_lead",
        "queued" => "queued",
        "stuck" => "stuck",
        "done" => "done",
        "stopped" => "stopped",
        "failed" => "failed",
        _ => "unknown",
    }
}

/// Whether a stop would stop a helper with this stored state, as `stop_member` decides: only while its team runs, and
/// not one already stopped or that couldn't start (`failed` without `failed_run`). A helper whose run failed keeps its
/// seat until it is stopped, so a page that hid Stop for every failed helper left that seat taken (GW-I-260).
pub(crate) fn can_stop(team_running: bool, stored: &str, failed_run: bool) -> bool {
    team_running
        && match stored {
            "stopped" => false,
            "failed" => failed_run,
            _ => true,
        }
}

/// A lead's helpers that need its person, as the task list shows them: stuck and failed ones while the team runs, none
/// once it has stopped, since nothing in an ended team waits on anyone (GW-U-169; before 17.0, an ended team's
/// "1 failed" stayed red in the list for good).
pub(crate) fn attention(team_running: bool, stuck: u32, failed: u32) -> Attention {
    if team_running {
        Attention { stuck, failed }
    } else {
        Attention {
            stuck: 0,
            failed: 0,
        }
    }
}

/// A queued helper's place in the slot queue, counted from 1 (1 is next), as the team answer gives it; none while it
/// waits for anything else: its turn on the workspace (a helper on the lead's files), its own sandbox or a Desktop
/// (GW-U-199). A position without a reason is an older gateway's slot place.
#[must_use]
pub fn slot_place(position: tqa_types::QueuePosition) -> Option<u32> {
    match position.reason() {
        None | Some(tqa_types::QueueReason::Slot) => position.ahead().checked_add(1),
        Some(_) => None,
    }
}

#[cfg(test)]
#[path = "place_tests.rs"]
mod tests;
