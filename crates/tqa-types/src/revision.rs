//! The protocol revision: which additions to protocol 1 a runtime can parse (ADR-0026).
//! Any change the gateway sends that an older runtime cannot parse raises [`PROTOCOL_REVISION`].

use crate::{MIN_RUNTIME_REVISION, PROTOCOL_REVISION};

/// One protocol revision and the wire field it added.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolRevision {
    /// The revision number, counted from 1.
    pub number: u16,
    /// What the revision added, for people.
    pub adds: &'static str,
    /// The schema definitions and the property or frame type each gained; empty only for the base.
    pub schema: &'static [(&'static str, &'static str)],
    /// The properties and frame types it took away, from this or an earlier revision (ADR-0035 changed revision 9
    /// in place before any release).
    pub removes: &'static [(&'static str, &'static str)],
}

/// Every revision in order; its length is the current revision, so a raise needs a new row.
pub const PROTOCOL_REVISIONS: [ProtocolRevision; PROTOCOL_REVISION as usize] = [
    ProtocolRevision {
        number: 1,
        adds: "the base protocol",
        schema: &[],
        removes: &[],
    },
    ProtocolRevision {
        number: 2,
        adds: "workspace.push in session.config (ADR-0023)",
        schema: &[("WorkspaceSnapshot", "push")],
        removes: &[],
    },
    ProtocolRevision {
        number: 3,
        adds: "the route's max_request_bytes",
        schema: &[("RouteSnapshot", "max_request_bytes")],
        removes: &[],
    },
    ProtocolRevision {
        number: 4,
        adds: "fallback_routes in session.config and session.update (ADR-0025)",
        schema: &[("ClientFrame", "fallback_routes")],
        removes: &[],
    },
    ProtocolRevision {
        number: 5,
        adds: "reasoning.state events with their opaque item in the runtime history (ADR-0027)",
        schema: &[("Event", "item")],
        removes: &[],
    },
    ProtocolRevision {
        number: 6,
        adds: "the route's reasoning budget table for protocols that take a thinking budget",
        schema: &[("ReasoningSnapshot", "budget_tokens")],
        removes: &[],
    },
    ProtocolRevision {
        number: 7,
        adds: "MCP servers and the route's cache hint in session.config; workspace request and cancel frames on the link (ADR-0011, ADR-0030)",
        schema: &[
            ("ClientFrame", "mcp"),
            ("RouteSnapshot", "cache_hint"),
            ("ClientFrame", "workspace.request"),
            ("ClientFrame", "workspace.cancel"),
        ],
        removes: &[],
    },
    ProtocolRevision {
        number: 8,
        adds: "the agent profile in session.config, the template version on the environment and environment.changed (ADR-0012, ADR-0032)",
        schema: &[
            ("ClientFrame", "profile"),
            ("Environment", "version"),
            ("Event", "version"),
        ],
        removes: &[],
    },
    ProtocolRevision {
        number: 9,
        adds: "the decision engine in session.config, the decision and decision.fallback events, and the decision_stop, decision_engine_unavailable and decision_engine_budget_spent error codes (ADR-0013); the gateway's revision and the task's network allow-list in session.config; it removes the policy, the workspace's push settings, the connector's permission class and the approval frames, adds the approvals_removed error code (ADR-0035), and raises the floor to 9",
        schema: &[
            ("ClientFrame", "decision_engine"),
            ("Event", "decision"),
            ("Event", "decision.fallback"),
            ("ErrorCode", "decision_stop"),
            ("ErrorCode", "decision_engine_unavailable"),
            ("ErrorCode", "decision_engine_budget_spent"),
            ("ErrorCode", "approvals_removed"),
            ("ClientFrame", "gateway_revision"),
            ("ClientFrame", "network"),
        ],
        removes: &[
            ("ClientFrame", "policy"),
            ("WorkspaceSnapshot", "push"),
            ("McpServer", "class"),
            ("ClientFrame", "permission.decision"),
            ("ClientFrame", "permission.forward"),
            ("ClientFrame", "session.park"),
        ],
    },
    ProtocolRevision {
        number: 10,
        adds: "the reply_cut error code: a reply the model ended at its output limit while the run's budget was left; the Git push failure key_use_unrecorded: a push that didn't run because its key's use couldn't be recorded (D4); and the route snapshot's first_response_secs, how long the relay waits for the model's first byte. It raises the floor to 10, since an older runtime can't read a history that holds reply_cut or parse the relay's new code. A network entry's host may also be a bare '*', any website on its ports (the open_web preset), and an environment may declare the capability data: Python's data libraries pandas, matplotlib and openpyxl, up to six capabilities. A queue position's reason may be own_sandbox: the run waits only for its owner's own idle sandbox to retire, so a client shows it as starting. A failed tool call's tool.end may say why as data (failure: a code and, for a refused destination, the target), so a client words it itself",
        schema: &[
            ("ErrorCode", "reply_cut"),
            ("GitPushFailure", "key_use_unrecorded"),
            ("RouteSnapshot", "first_response_secs"),
        ],
        removes: &[],
    },
    ProtocolRevision {
        number: 11,
        adds: "the run's host keys on the user.message the gateway delivers (ADR-0037 §3): for each key its environment variable, host, port, path prefix, scheme, user name and the run's placeholder, never its value; and in session.config the install CA the sandbox trusts for keyed hosts and the keys of a run in progress, newly minted for this sandbox, which a resumed run gets back. They go only to a runtime announcing 11; the floor stays 10 (guardrails §8), and a turn whose sender has keys on an older runtime gets the host_keys_unsupported notice; and the live host_key.used and host_key.refused frames, gateway to clients only",
        schema: &[
            ("ClientFrame", "host_keys"),
            ("ClientFrame", "host_ca"),
            ("ClientFrame", "run_host_keys"),
            ("ErrorCode", "host_keys_unsupported"),
            ("ServerFrame", "host_key.used"),
            ("ServerFrame", "host_key.refused"),
        ],
        removes: &[],
    },
    ProtocolRevision {
        number: 12,
        adds: "the lead's brief on an agent team member's first turn (ADR-0034): user.message's lead_brief (the member's role, workspace mode and the lead's brief), which the runtime journals on message.user and gives the model after the gateway's frame inside the untrusted-content marker. Only a member's turn carries it, and a member whose runtime announces less than 12 is refused at the link (runtime_too_old); the floor stays 10",
        schema: &[("ClientFrame", "lead_brief"), ("Event", "lead_brief")],
        removes: &[],
    },
    ProtocolRevision {
        number: 13,
        adds: "the gateway's wake of an agent team reader (ADR-0034 Waking): user.message's team_wake (the reader's place in the log; new team messages wait after it, whoever wrote them), which the runtime journals on message.user so a client shows the gateway's notice and never a person's message. The turn's text is unchanged for the model. A team's lead or member whose runtime announces less than 13 is refused at the link (runtime_too_old); the floor stays 10",
        schema: &[
            ("ClientFrame", "team_wake"),
            ("Event", "team_wake"),
            ("TeamWake", "after"),
        ],
        removes: &[],
    },
    ProtocolRevision {
        number: 14,
        adds: "the Desktop (ADR-0039): an environment may declare the capability desktop, a graphical desktop the agent operates with the computer tool, up to seven capabilities; and a queue position's reason may be desktop_sessions, a run waiting for a free Desktop under desktop.max_sessions. A computer call that acted on an element by ref carries it on its tool.result as target (a role from a closed list of 22 and a name of at most 200 characters, for a client's step line; never in the model's input), and a computer action's tool.result says which of its evidence screenshots were kept (evidence: before, after; a screenshot call's Ok result has after only). The new event screen.capture names one stored screenshot (its call, phase, store id, width, height, size, SHA-256, whether the model was sent it and whether it is marked), journaled once the store holds it: a before capture ahead of its call's tool.start, the others ahead of its tool.result. The route snapshot's pixel_grounding (default off) offers the computer tool's coordinate actions, and session.config's computer_limits carries the tool's per-run caps on screenshots (1 to 1000, default 200) and actions (1 to 5000, default 1000). Sign out everywhere sends screen.sign_out to the runtime, which empties the desktop browser's profile and answers screen.sign_out.done (ok, or a reason from a closed list). After a desktop run's end the runtime empties that profile, starts the browser again and sends screen.reset (ok); the gateway grants a Take over that cancelled the run only after it, waiting at most 10 s. The gateway's own screen notices go to clients only (screen.notice: taken over, handed back, signed out, sign-out failed). A screenshot reaches the gateway's store as screen.upload chunks keyed by its SHA-256, answered by screen.stored (ok, or too_large, quota_full, bad_image or store_failed). A conversation whose environment declares desktop is refused at the link below 14 (runtime_too_old); the floor stays 10",
        schema: &[
            ("ClientFrame", "screen.sign_out"),
            ("ServerFrame", "screen.sign_out.done"),
            ("ServerFrame", "screen.reset"),
            ("ServerFrame", "screen.notice"),
            ("ServerFrame", "screen.upload"),
            ("ClientFrame", "screen.stored"),
            ("QueueReason", "desktop_sessions"),
            ("Event", "target"),
            ("ScreenTarget", "role"),
            ("ScreenTarget", "name"),
            ("Event", "evidence"),
            ("ScreenEvidence", "before"),
            ("ScreenEvidence", "after"),
            ("Event", "screen.capture"),
            ("RouteSnapshot", "pixel_grounding"),
            ("ClientFrame", "computer_limits"),
        ],
        removes: &[],
    },
];

/// A revision header or label that is not a canonical decimal from 1 to 65535.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("malformed protocol revision")]
pub struct InvalidRevision;

/// Reads a revision from a header or an image label; an absent value is revision 1.
///
/// # Errors
/// Returns [`InvalidRevision`] for anything but a canonical decimal from 1 to 65535.
pub fn parse_revision(value: Option<&str>) -> Result<u16, InvalidRevision> {
    let Some(value) = value else {
        return Ok(1);
    };
    if value.is_empty() || value.starts_with('0') || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(InvalidRevision);
    }
    value.parse().map_err(|_| InvalidRevision)
}

/// A runtime whose revision is below the gateway's floor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("runtime protocol revision {has} is older than the required {needs}")]
pub struct RuntimeTooOld {
    /// The lowest revision this build accepts.
    pub needs: u16,
    /// The runtime's revision.
    pub has: u16,
}

/// Accepts a runtime at or above [`MIN_RUNTIME_REVISION`]; newer runtimes are accepted.
///
/// # Errors
/// Returns [`RuntimeTooOld`] with both revisions below the floor.
pub const fn check_runtime_revision(has: u16) -> Result<(), RuntimeTooOld> {
    if has < MIN_RUNTIME_REVISION {
        Err(RuntimeTooOld {
            needs: MIN_RUNTIME_REVISION,
            has,
        })
    } else {
        Ok(())
    }
}
