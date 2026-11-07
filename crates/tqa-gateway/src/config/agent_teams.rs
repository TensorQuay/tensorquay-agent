//! `[agent_teams]`: the install switch (ADR-0034, decided 2 Oct 2026) and the team limits. Agent teams are off unless
//! `enabled = true`: no task gets a team tool and every team route answers `teams_disabled`. The limits are file
//! settings for now; the administrator's runtime row for them comes with the settings page (DEV-PLAN Phase 10, slice 6).
use crate::GatewayError;
use serde::Deserialize;

/// Members per lead at a time: the default, and the most the file may set (ADR-0034 point 3).
const DEFAULT_MAX_MEMBERS: u32 = 3;
const MAX_MAX_MEMBERS: u32 = 8;
/// Tokens for one agent team, lead and members together (point 8).
const DEFAULT_TOKEN_BUDGET: u64 = 2_000_000;
/// Wall time from the first member's start: the default, and at most a day.
const DEFAULT_DEADLINE_MINUTES: u32 = 120;
const MAX_DEADLINE_MINUTES: u32 = 24 * 60;
/// Liveness (point 9, slice 4): a member idle, a member stuck, and the whole team idle, in minutes; each 1 through a
/// day (the lead's decision of 2 Oct 2026).
const DEFAULT_MEMBER_IDLE_MINUTES: u32 = 10;
const DEFAULT_MEMBER_STUCK_MINUTES: u32 = 15;
const DEFAULT_IDLE_STOP_MINUTES: u32 = 30;
const MAX_LIVENESS_MINUTES: u32 = 24 * 60;
/// The share of the budget, in percent, past which helpers are refused and the lead has the rest to write up
/// (GW-I-304, c16k B9b): 1 through 99.
const DEFAULT_HELPERS_STOP_PERCENT: u32 = 90;

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct AgentTeamsInput {
    enabled: bool,
    max_members: Option<u32>,
    token_budget: Option<u64>,
    deadline_minutes: Option<u32>,
    member_route: Option<String>,
    member_idle_minutes: Option<u32>,
    member_stuck_minutes: Option<u32>,
    idle_stop_minutes: Option<u32>,
    helpers_stop_percent: Option<u32>,
}

/// Whether this installation has agent teams, and their limits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentTeams {
    /// Off by default.
    pub(crate) enabled: bool,
    /// Members per lead at a time, before admission lowers it ([`crate::teams::cap`]).
    pub(crate) max_members: u32,
    pub(crate) token_budget: u64,
    pub(crate) deadline_minutes: u32,
    /// The model route of a member started without a profile; unset, the lead's own.
    pub(crate) member_route: Option<String>,
    /// A member whose run ended without a report is idle after this long (`member_idle` notice).
    pub(crate) member_idle_minutes: u32,
    /// A member whose active run made no progress for this long is stuck (`member_stuck` notice).
    pub(crate) member_stuck_minutes: u32,
    /// The team stops after this long with no message and no active run.
    pub(crate) idle_stop_minutes: u32,
    /// Past this share of the budget, in percent, helpers are refused and the lead writes up with the rest.
    pub(crate) helpers_stop_percent: u32,
}

impl Default for AgentTeams {
    fn default() -> Self {
        Self {
            enabled: false,
            max_members: DEFAULT_MAX_MEMBERS,
            token_budget: DEFAULT_TOKEN_BUDGET,
            deadline_minutes: DEFAULT_DEADLINE_MINUTES,
            member_route: None,
            member_idle_minutes: DEFAULT_MEMBER_IDLE_MINUTES,
            member_stuck_minutes: DEFAULT_MEMBER_STUCK_MINUTES,
            idle_stop_minutes: DEFAULT_IDLE_STOP_MINUTES,
            helpers_stop_percent: DEFAULT_HELPERS_STOP_PERCENT,
        }
    }
}

/// The section with its defaults.
///
/// # Errors
/// A limit out of its bounds, or an empty `member_route`.
pub(super) fn agent_teams(input: AgentTeamsInput) -> Result<AgentTeams, GatewayError> {
    let defaults = AgentTeams::default();
    let teams = AgentTeams {
        enabled: input.enabled,
        max_members: input.max_members.unwrap_or(defaults.max_members),
        token_budget: input.token_budget.unwrap_or(defaults.token_budget),
        deadline_minutes: input.deadline_minutes.unwrap_or(defaults.deadline_minutes),
        member_route: input.member_route,
        member_idle_minutes: input
            .member_idle_minutes
            .unwrap_or(defaults.member_idle_minutes),
        member_stuck_minutes: input
            .member_stuck_minutes
            .unwrap_or(defaults.member_stuck_minutes),
        idle_stop_minutes: input
            .idle_stop_minutes
            .unwrap_or(defaults.idle_stop_minutes),
        helpers_stop_percent: input
            .helpers_stop_percent
            .unwrap_or(defaults.helpers_stop_percent),
    };
    if !(1..=99).contains(&teams.helpers_stop_percent) {
        return Err(GatewayError::Configuration(
            "agent_teams.helpers_stop_percent must be 1 through 99",
        ));
    }
    if !(1..=MAX_MAX_MEMBERS).contains(&teams.max_members) {
        return Err(GatewayError::Configuration(
            "agent_teams.max_members must be 1 through 8",
        ));
    }
    if teams.token_budget == 0 {
        return Err(GatewayError::Configuration(
            "agent_teams.token_budget must be above 0",
        ));
    }
    if !(1..=MAX_DEADLINE_MINUTES).contains(&teams.deadline_minutes) {
        return Err(GatewayError::Configuration(
            "agent_teams.deadline_minutes must be 1 through 1440",
        ));
    }
    for (value, refusal) in [
        (
            teams.member_idle_minutes,
            "agent_teams.member_idle_minutes must be 1 through 1440",
        ),
        (
            teams.member_stuck_minutes,
            "agent_teams.member_stuck_minutes must be 1 through 1440",
        ),
        (
            teams.idle_stop_minutes,
            "agent_teams.idle_stop_minutes must be 1 through 1440",
        ),
    ] {
        if !(1..=MAX_LIVENESS_MINUTES).contains(&value) {
            return Err(GatewayError::Configuration(refusal));
        }
    }
    if teams.member_route.as_deref().is_some_and(str::is_empty) {
        return Err(GatewayError::Configuration(
            "agent_teams.member_route must name a route",
        ));
    }
    Ok(teams)
}

#[cfg(test)]
mod tests {
    use super::{AgentTeams, AgentTeamsInput, agent_teams};

    #[derive(serde::Deserialize)]
    struct File {
        #[serde(default)]
        agent_teams: AgentTeamsInput,
    }

    fn parse(text: &str) -> Result<Result<AgentTeams, String>, toml::de::Error> {
        toml::from_str::<File>(text)
            .map(|file| agent_teams(file.agent_teams).map_err(|error| error.to_string()))
    }

    #[test]
    fn gw_u_127_agent_teams_are_off_unless_enabled() -> Result<(), toml::de::Error> {
        let enabled = |text| parse(text).map(|teams| teams.is_ok_and(|teams| teams.enabled));
        assert!(!enabled("")?);
        assert!(!enabled("[agent_teams]\n")?);
        assert!(!enabled("[agent_teams]\nenabled = false\n")?);
        assert!(enabled("[agent_teams]\nenabled = true\n")?);
        Ok(())
    }

    #[test]
    fn gw_u_127_an_unknown_key_is_refused() {
        let refused = parse("[agent_teams]\nenable = true\n")
            .err()
            .map(|error| error.to_string());
        assert!(refused.is_some_and(|message| message.contains("enable")));
    }

    #[test]
    fn gw_u_128_the_helpers_share_of_the_budget_is_1_through_99() -> Result<(), toml::de::Error> {
        // GW-I-304: past this share the helpers stop and the lead writes up with the rest.
        assert_eq!(AgentTeams::default().helpers_stop_percent, 90);
        let share =
            |text: &str| parse(text).map(|teams| teams.map(|teams| teams.helpers_stop_percent));
        assert_eq!(share("[agent_teams]\nhelpers_stop_percent = 1\n")?, Ok(1));
        assert_eq!(share("[agent_teams]\nhelpers_stop_percent = 99\n")?, Ok(99));
        for refused in ["helpers_stop_percent = 0", "helpers_stop_percent = 100"] {
            let said = share(&format!("[agent_teams]\n{refused}\n"))?;
            assert!(
                said.as_ref()
                    .is_err_and(|message| message.contains("helpers_stop_percent")),
                "{refused}: {said:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn gw_u_128_the_team_limits_have_defaults_and_bounds() -> Result<(), toml::de::Error> {
        assert_eq!(parse("")?, Ok(AgentTeams::default()));
        let defaults = AgentTeams::default();
        assert_eq!(
            (
                defaults.max_members,
                defaults.token_budget,
                defaults.deadline_minutes,
                defaults.member_route
            ),
            (3, 2_000_000, 120, None)
        );
        let set = parse(
            "[agent_teams]\nmax_members = 8\ntoken_budget = 1\ndeadline_minutes = 1440\nmember_route = \"local\"\n",
        )?;
        assert_eq!(
            set.map(|teams| (
                teams.max_members,
                teams.token_budget,
                teams.deadline_minutes,
                teams.member_route
            )),
            Ok((8, 1, 1440, Some("local".to_owned())))
        );
        assert_eq!(
            (
                defaults.member_idle_minutes,
                defaults.member_stuck_minutes,
                defaults.idle_stop_minutes
            ),
            (10, 15, 30)
        );
        let liveness = parse(
            "[agent_teams]\nmember_idle_minutes = 1\nmember_stuck_minutes = 1440\nidle_stop_minutes = 45\n",
        )?;
        assert_eq!(
            liveness.map(|teams| (
                teams.member_idle_minutes,
                teams.member_stuck_minutes,
                teams.idle_stop_minutes
            )),
            Ok((1, 1440, 45))
        );
        for (text, named) in [
            ("max_members = 0", "max_members"),
            ("max_members = 9", "max_members"),
            ("token_budget = 0", "token_budget"),
            ("deadline_minutes = 0", "deadline_minutes"),
            ("deadline_minutes = 1441", "deadline_minutes"),
            ("member_route = \"\"", "member_route"),
            ("member_idle_minutes = 0", "member_idle_minutes"),
            ("member_stuck_minutes = 1441", "member_stuck_minutes"),
            ("idle_stop_minutes = 0", "idle_stop_minutes"),
        ] {
            let refused = parse(&format!("[agent_teams]\n{text}\n"))?.err();
            assert!(
                refused.is_some_and(|message| message.contains(named)),
                "{text}"
            );
        }
        Ok(())
    }
}
