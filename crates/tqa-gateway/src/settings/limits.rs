//! Admin limit overrides (ADR-0028 slice 5): a closed list of limits an administrator may change, their bounds (the
//! file validator's own, `config/admission.rs` and `config/sandbox.rs`), the order between them, and the clamp applied at
//! load when a file edit leaves a stored value out of bounds. Pure: storage and the API build on it.
use crate::InvalidReason;
use serde_json::Value;

/// The limits an administrator may override. Everything else stays file-only (ADR-0028).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum LimitKey {
    Slots,
    PerUser,
    IdleSeconds,
    MaxLifetimeSeconds,
    McpCallsPerMinute,
    /// The agent teams' limits (ADR-0034 S6): listed and written only while agent teams are on.
    TeamMaxMembers,
    TeamTokenBudget,
    TeamDeadlineMinutes,
    MemberIdleMinutes,
    MemberStuckMinutes,
    TeamIdleStopMinutes,
    /// The Desktop's limits (ADR-0039): live desktop sandboxes at once, and how long action screenshots are kept.
    DesktopMaxSessions,
    DesktopScreenshotRetentionDays,
    /// The `computer` tool's per-run caps (ADR-0039 section 4): screenshots sent to the model, and actions taken.
    ComputerMaxScreenshotsPerRun,
    ComputerMaxActionsPerRun,
}

impl LimitKey {
    /// Every key, in the order the API lists them.
    pub(crate) const ALL: [Self; 15] = [
        Self::Slots,
        Self::PerUser,
        Self::IdleSeconds,
        Self::MaxLifetimeSeconds,
        Self::McpCallsPerMinute,
        Self::TeamMaxMembers,
        Self::TeamTokenBudget,
        Self::TeamDeadlineMinutes,
        Self::MemberIdleMinutes,
        Self::MemberStuckMinutes,
        Self::TeamIdleStopMinutes,
        Self::DesktopMaxSessions,
        Self::DesktopScreenshotRetentionDays,
        Self::ComputerMaxScreenshotsPerRun,
        Self::ComputerMaxActionsPerRun,
    ];

    /// The stable name, as in the configuration file and the API.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Slots => "admission.slots",
            Self::PerUser => "admission.per_user",
            Self::IdleSeconds => "sandbox.idle_seconds",
            Self::MaxLifetimeSeconds => "sandbox.max_lifetime_seconds",
            Self::McpCallsPerMinute => "mcp.calls_per_minute",
            Self::TeamMaxMembers => "agent_teams.max_members",
            Self::TeamTokenBudget => "agent_teams.token_budget",
            Self::TeamDeadlineMinutes => "agent_teams.deadline_minutes",
            Self::MemberIdleMinutes => "agent_teams.member_idle_minutes",
            Self::MemberStuckMinutes => "agent_teams.member_stuck_minutes",
            Self::TeamIdleStopMinutes => "agent_teams.idle_stop_minutes",
            Self::DesktopMaxSessions => "desktop.max_sessions",
            Self::DesktopScreenshotRetentionDays => "desktop.screenshot_retention_days",
            Self::ComputerMaxScreenshotsPerRun => "computer.max_screenshots_per_run",
            Self::ComputerMaxActionsPerRun => "computer.max_actions_per_run",
        }
    }

    /// One of the agent teams' limits, which exist for the API only while agent teams are on.
    pub(crate) const fn is_team(self) -> bool {
        matches!(
            self,
            Self::TeamMaxMembers
                | Self::TeamTokenBudget
                | Self::TeamDeadlineMinutes
                | Self::MemberIdleMinutes
                | Self::MemberStuckMinutes
                | Self::TeamIdleStopMinutes
        )
    }

    /// The key with exactly this name.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.as_str() == name)
    }

    /// The unit the value counts.
    pub(crate) const fn unit(self) -> &'static str {
        match self {
            Self::Slots | Self::PerUser | Self::DesktopMaxSessions => "sandboxes",
            Self::IdleSeconds | Self::MaxLifetimeSeconds => "seconds",
            Self::McpCallsPerMinute => "calls",
            Self::TeamMaxMembers => "helpers",
            Self::TeamTokenBudget => "tokens",
            Self::TeamDeadlineMinutes
            | Self::MemberIdleMinutes
            | Self::MemberStuckMinutes
            | Self::TeamIdleStopMinutes => "minutes",
            Self::DesktopScreenshotRetentionDays => "days",
            Self::ComputerMaxScreenshotsPerRun => "screenshots",
            Self::ComputerMaxActionsPerRun => "actions",
        }
    }

    /// When a change takes effect: admission limits at the next admission pass, sandbox limits for the next sandbox
    /// created (a running one keeps what it was created with, ADR-0028), the connector rate at the next proxied call;
    /// a team's helper cap from its next helper, its budget and deadline for the next team, and the liveness times at
    /// the reconciler's next check. The `computer` tool's caps reach a runtime in `session.config`, so from its next
    /// session: a run resumed after a reconnect gets the new caps, counted against what it has spent.
    pub(crate) const fn applies(self) -> &'static str {
        match self {
            Self::Slots | Self::PerUser | Self::DesktopMaxSessions => "next_admission",
            Self::McpCallsPerMinute => "next_call",
            Self::IdleSeconds | Self::MaxLifetimeSeconds => "next_sandbox",
            Self::TeamMaxMembers => "next_member",
            Self::TeamTokenBudget | Self::TeamDeadlineMinutes => "next_team",
            Self::MemberIdleMinutes | Self::MemberStuckMinutes | Self::TeamIdleStopMinutes => {
                "next_check"
            }
            Self::DesktopScreenshotRetentionDays => "next_purge",
            Self::ComputerMaxScreenshotsPerRun | Self::ComputerMaxActionsPerRun => "next_session",
        }
    }
}

/// The effective value of every overridable limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Limits {
    pub slots: u32,
    pub per_user: u32,
    pub idle_seconds: u32,
    pub max_lifetime_seconds: u32,
    /// Messages one user may send through the connector proxy per minute (ADR-0031), refilled continuously.
    pub mcp_calls_per_minute: u32,
    /// Helpers per lead at a time, before admission lowers it (`teams::cap`).
    pub team_max_members: u32,
    pub team_token_budget: u64,
    pub team_deadline_minutes: u32,
    pub member_idle_minutes: u32,
    pub member_stuck_minutes: u32,
    pub team_idle_stop_minutes: u32,
    /// Live desktop sandboxes at once; another waits for a slot (`desktop_sessions`).
    pub desktop_max_sessions: u32,
    /// Days an action screenshot is kept (ADR-0039 section 9).
    pub desktop_screenshot_retention_days: u32,
    /// The screenshots a run of the `computer` tool sends the model, and the actions it takes (ADR-0039 section 4).
    pub computer_max_screenshots_per_run: u32,
    pub computer_max_actions_per_run: u32,
}

impl Limits {
    /// The value of `key`.
    pub(crate) const fn get(&self, key: LimitKey) -> u64 {
        match key {
            LimitKey::Slots => self.slots as u64,
            LimitKey::PerUser => self.per_user as u64,
            LimitKey::IdleSeconds => self.idle_seconds as u64,
            LimitKey::MaxLifetimeSeconds => self.max_lifetime_seconds as u64,
            LimitKey::McpCallsPerMinute => self.mcp_calls_per_minute as u64,
            LimitKey::TeamMaxMembers => self.team_max_members as u64,
            LimitKey::TeamTokenBudget => self.team_token_budget,
            LimitKey::TeamDeadlineMinutes => self.team_deadline_minutes as u64,
            LimitKey::MemberIdleMinutes => self.member_idle_minutes as u64,
            LimitKey::MemberStuckMinutes => self.member_stuck_minutes as u64,
            LimitKey::TeamIdleStopMinutes => self.team_idle_stop_minutes as u64,
            LimitKey::DesktopMaxSessions => self.desktop_max_sessions as u64,
            LimitKey::DesktopScreenshotRetentionDays => {
                self.desktop_screenshot_retention_days as u64
            }
            LimitKey::ComputerMaxScreenshotsPerRun => self.computer_max_screenshots_per_run as u64,
            LimitKey::ComputerMaxActionsPerRun => self.computer_max_actions_per_run as u64,
        }
    }

    /// These limits with `key` set to `value`. A value too large for a 32-bit limit saturates, and `check` refuses it.
    #[must_use]
    pub(crate) fn with(mut self, key: LimitKey, value: u64) -> Self {
        let small = u32::try_from(value).unwrap_or(u32::MAX);
        match key {
            LimitKey::Slots => self.slots = small,
            LimitKey::PerUser => self.per_user = small,
            LimitKey::IdleSeconds => self.idle_seconds = small,
            LimitKey::MaxLifetimeSeconds => self.max_lifetime_seconds = small,
            LimitKey::McpCallsPerMinute => self.mcp_calls_per_minute = small,
            LimitKey::TeamMaxMembers => self.team_max_members = small,
            LimitKey::TeamTokenBudget => self.team_token_budget = value,
            LimitKey::TeamDeadlineMinutes => self.team_deadline_minutes = small,
            LimitKey::MemberIdleMinutes => self.member_idle_minutes = small,
            LimitKey::MemberStuckMinutes => self.member_stuck_minutes = small,
            LimitKey::TeamIdleStopMinutes => self.team_idle_stop_minutes = small,
            LimitKey::DesktopMaxSessions => self.desktop_max_sessions = small,
            LimitKey::DesktopScreenshotRetentionDays => {
                self.desktop_screenshot_retention_days = small;
            }
            LimitKey::ComputerMaxScreenshotsPerRun => self.computer_max_screenshots_per_run = small,
            LimitKey::ComputerMaxActionsPerRun => self.computer_max_actions_per_run = small,
        }
        self
    }
}

/// An inclusive range a limit's value must fall in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Bounds {
    pub min: u64,
    pub max: u64,
}

/// One day, the idle bound in `config/sandbox.rs`.
const DAY_SECONDS: u64 = 86_400;
/// A sandbox lives at least a minute and at most a week (`config/sandbox.rs`).
const MIN_LIFETIME_SECONDS: u64 = 60;
const MAX_LIFETIME_SECONDS: u64 = 7 * DAY_SECONDS;
/// The connector rate bound in `config/mcp.rs`: at most a hundred messages a second per user.
const MAX_MCP_CALLS_PER_MINUTE: u64 = 6_000;
/// An agent team's bounds (ADR-0034 S6 contract): at most 8 helpers, a token budget up to 100 million, and minutes up
/// to a day. An administrator's floors are higher than the file's: a budget of 10,000 tokens and a 5-minute deadline.
const MAX_TEAM_MEMBERS: u64 = 8;
const MAX_TEAM_TOKENS: u64 = 100_000_000;
const MIN_ADMIN_TEAM_TOKENS: u64 = 10_000;
const MAX_TEAM_MINUTES: u64 = 24 * 60;
const MIN_ADMIN_DEADLINE_MINUTES: u64 = 5;
/// Action screenshots are kept a year at most (ADR-0039 section 9).
const MAX_RETENTION_DAYS: u64 = 365;

/// The bounds for `key`: the file's own. `slots` is bounded by the file-only `sandbox.max_live`, and `per_user` by the
/// effective `slots` in `limits`.
pub(crate) fn bounds(key: LimitKey, limits: &Limits, max_live: u32) -> Bounds {
    let (min, max) = match key {
        LimitKey::Slots => (1, u64::from(max_live)),
        // At most the slots, as the per-user cap: a lowered `slots` lowers the Desktop's at load too.
        LimitKey::PerUser | LimitKey::DesktopMaxSessions => (1, u64::from(limits.slots)),
        LimitKey::IdleSeconds => (1, DAY_SECONDS),
        LimitKey::MaxLifetimeSeconds => (MIN_LIFETIME_SECONDS, MAX_LIFETIME_SECONDS),
        LimitKey::McpCallsPerMinute => (1, MAX_MCP_CALLS_PER_MINUTE),
        // The helper cap's own bound; its order under the admission limits is checked only when it is written
        // ([`check_write`]), so lowering the slots is never refused for it: the effective cap drops (`teams::cap`).
        LimitKey::TeamMaxMembers => (1, MAX_TEAM_MEMBERS),
        LimitKey::TeamTokenBudget => (1, MAX_TEAM_TOKENS),
        LimitKey::TeamDeadlineMinutes
        | LimitKey::MemberIdleMinutes
        | LimitKey::MemberStuckMinutes
        | LimitKey::TeamIdleStopMinutes => (1, MAX_TEAM_MINUTES),
        LimitKey::DesktopScreenshotRetentionDays => (1, MAX_RETENTION_DAYS),
        // The protocol's own bounds (`tqa_types::ComputerLimits`), so session.config can always carry the value.
        LimitKey::ComputerMaxScreenshotsPerRun => {
            (1, u64::from(crate::config::MAX_SCREENSHOTS_PER_RUN))
        }
        LimitKey::ComputerMaxActionsPerRun => (1, u64::from(crate::config::MAX_ACTIONS_PER_RUN)),
    };
    Bounds { min, max }
}

/// The most helpers a lead may have beside it under the admission limits: the slots and the per-user cap, less one
/// for the lead.
fn helper_room(limits: &Limits) -> u64 {
    u64::from(limits.slots.min(limits.per_user).saturating_sub(1))
}

/// The bounds an administrator's write must meet: the file's, with the team limits' floors (a budget of 10,000
/// tokens, a 5-minute deadline) and the helper cap under the admission limits. (The sizes, whose floors kept one click
/// from making every sandbox fail, moved to each environment with the environment sizes design.)
pub(crate) fn admin_bounds(key: LimitKey, limits: &Limits, max_live: u32) -> Bounds {
    let file = bounds(key, limits, max_live);
    match key {
        LimitKey::TeamMaxMembers => Bounds {
            min: file.min,
            max: file.max.min(helper_room(limits)),
        },
        LimitKey::TeamTokenBudget => Bounds {
            min: MIN_ADMIN_TEAM_TOKENS,
            ..file
        },
        LimitKey::TeamDeadlineMinutes => Bounds {
            min: MIN_ADMIN_DEADLINE_MINUTES,
            ..file
        },
        _ => file,
    }
}

/// Checks an administrator's write of `key`: its value against [`admin_bounds`], then the whole set against the file's
/// bounds and order.
///
/// # Errors
/// As [`check`], and `limit_out_of_range` for a written value below its floor.
pub(crate) fn check_write(
    limits: &Limits,
    key: LimitKey,
    max_live: u32,
) -> Result<(), (LimitKey, InvalidReason)> {
    let value = limits.get(key);
    if value < admin_bounds(key, limits, max_live).min {
        return Err((key, InvalidReason::LimitOutOfRange));
    }
    check(limits, max_live)?;
    // A helper cap above what the admission limits leave beside the lead (ADR-0034 S6: `limit_order`).
    if key == LimitKey::TeamMaxMembers && value > helper_room(limits) {
        return Err((key, InvalidReason::LimitOrder));
    }
    Ok(())
}

/// A limit's value from the request: a non-negative JSON integer. Bounds are `check`'s.
pub(crate) fn parse_value(value: &Value) -> Option<u64> {
    value.as_u64()
}

/// Checks every value against its bounds, `slots` before `per_user`. `per_user` above `slots` is an order refusal
/// whichever of the two moved.
///
/// # Errors
/// The first refused key and its reason: `limit_out_of_range` or `limit_order`.
pub(crate) fn check(limits: &Limits, max_live: u32) -> Result<(), (LimitKey, InvalidReason)> {
    for key in LimitKey::ALL {
        let value = limits.get(key);
        let range = bounds(key, limits, max_live);
        if value < range.min {
            return Err((key, InvalidReason::LimitOutOfRange));
        }
        if value > range.max {
            let reason = if matches!(key, LimitKey::PerUser | LimitKey::DesktopMaxSessions) {
                InvalidReason::LimitOrder
            } else {
                InvalidReason::LimitOutOfRange
            };
            return Err((key, reason));
        }
    }
    Ok(())
}

/// One value the load moved into its bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Clamped {
    pub key: LimitKey,
    pub from: u64,
    pub to: u64,
}

/// Moves every value into its bounds, in `check`'s order (so a lowered `slots` also lowers `per_user`), and reports
/// what moved. Used at load, when a file edit left a stored value out of bounds (ADR-0028).
pub(crate) fn clamp(mut limits: Limits, max_live: u32) -> (Limits, Vec<Clamped>) {
    let mut changes = Vec::new();
    for key in LimitKey::ALL {
        let value = limits.get(key);
        let range = bounds(key, &limits, max_live);
        let to = value.clamp(range.min, range.max);
        if to != value {
            limits = limits.with(key, to);
            changes.push(Clamped {
                key,
                from: value,
                to,
            });
        }
    }
    (limits, changes)
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;
