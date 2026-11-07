//! GW-U-035: the overridable limits, their bounds, order and value parsing, and the clamp at load.
use super::{
    Clamped, LimitKey, Limits, admin_bounds, bounds, check, check_write, clamp, parse_value,
};
use crate::InvalidReason;
use serde_json::json;

fn file() -> Limits {
    Limits {
        slots: 8,
        per_user: 4,
        idle_seconds: 600,
        max_lifetime_seconds: 86_400,
        mcp_calls_per_minute: 60,
        team_max_members: 3,
        team_token_budget: 2_000_000,
        team_deadline_minutes: 120,
        member_idle_minutes: 10,
        member_stuck_minutes: 15,
        team_idle_stop_minutes: 30,
        desktop_max_sessions: 2,
        desktop_screenshot_retention_days: 30,
        computer_max_screenshots_per_run: 200,
        computer_max_actions_per_run: 1_000,
    }
}

#[test]
fn gw_u_035_the_closed_list_has_its_keys_with_stable_names() {
    let names: Vec<&str> = LimitKey::ALL.iter().map(|key| key.as_str()).collect();
    assert_eq!(
        names,
        [
            "admission.slots",
            "admission.per_user",
            "sandbox.idle_seconds",
            "sandbox.max_lifetime_seconds",
            "mcp.calls_per_minute",
            // The agent teams' limits (ADR-0034 S6, GW-U-131).
            "agent_teams.max_members",
            "agent_teams.token_budget",
            "agent_teams.deadline_minutes",
            "agent_teams.member_idle_minutes",
            "agent_teams.member_stuck_minutes",
            "agent_teams.idle_stop_minutes",
            // The Desktop's limits (ADR-0039, GW-U-146).
            "desktop.max_sessions",
            "desktop.screenshot_retention_days",
            // The computer tool's per-run caps (ADR-0039 section 4, GW-U-152).
            "computer.max_screenshots_per_run",
            "computer.max_actions_per_run",
        ]
    );
    for key in LimitKey::ALL {
        assert_eq!(LimitKey::parse(key.as_str()), Some(key));
    }
    for unknown in [
        "",
        "admission",
        "sandbox.max_live",
        "ADMISSION.SLOTS",
        "admission.slots ",
        "mcp_calls_per_minute",
        "mcp.calls",
        // An environment's size since the environment sizes design.
        "sandbox.cpu_millicores",
        "sandbox.memory_bytes",
        "sandbox.pids",
    ] {
        assert_eq!(LimitKey::parse(unknown), None, "{unknown:?}");
    }
}

#[test]
fn gw_u_035_units_and_when_each_limit_applies() {
    let expected = [
        (LimitKey::Slots, "sandboxes", "next_admission"),
        (LimitKey::PerUser, "sandboxes", "next_admission"),
        (LimitKey::IdleSeconds, "seconds", "next_sandbox"),
        (LimitKey::MaxLifetimeSeconds, "seconds", "next_sandbox"),
        (LimitKey::McpCallsPerMinute, "calls", "next_call"),
    ];
    for (key, unit, applies) in expected {
        assert_eq!((key.unit(), key.applies()), (unit, applies), "{key:?}");
    }
}

#[test]
fn gw_u_035_bounds_are_the_files_own_and_per_user_follows_slots() {
    let limits = file();
    let expected = [
        (LimitKey::Slots, 1, 16),
        (LimitKey::PerUser, 1, 8),
        (LimitKey::IdleSeconds, 1, 86_400),
        (LimitKey::MaxLifetimeSeconds, 60, 7 * 86_400),
        (LimitKey::McpCallsPerMinute, 1, 6_000),
    ];
    for (key, min, max) in expected {
        let found = bounds(key, &limits, 16);
        assert_eq!((found.min, found.max), (min, max), "{key:?}");
    }
}

#[test]
fn gw_u_035_values_are_non_negative_json_integers() {
    assert_eq!(parse_value(&json!(0)), Some(0));
    assert_eq!(parse_value(&json!(42)), Some(42));
    assert_eq!(parse_value(&json!(u64::MAX)), Some(u64::MAX));
    for bad in [
        json!(-1),
        json!(1.5),
        json!("1"),
        json!(null),
        json!(true),
        json!([1]),
        json!({"v": 1}),
    ] {
        assert_eq!(parse_value(&bad), None, "{bad}");
    }
}

#[test]
fn gw_u_035_a_set_is_checked_value_by_value_and_in_order() {
    assert_eq!(check(&file(), 16), Ok(()));
    let with = |key: LimitKey, value: u64| file().with(key, value);
    assert_eq!(
        check(&with(LimitKey::Slots, 16), 16),
        Ok(()),
        "exactly max_live"
    );
    assert_eq!(
        check(&with(LimitKey::Slots, 17), 16),
        Err((LimitKey::Slots, InvalidReason::LimitOutOfRange))
    );
    assert_eq!(
        check(&with(LimitKey::Slots, 0), 16),
        Err((LimitKey::Slots, InvalidReason::LimitOutOfRange))
    );
    assert_eq!(
        check(&with(LimitKey::McpCallsPerMinute, 6_001), 16),
        Err((LimitKey::McpCallsPerMinute, InvalidReason::LimitOutOfRange))
    );
    assert_eq!(
        check(&with(LimitKey::McpCallsPerMinute, 0), 16),
        Err((LimitKey::McpCallsPerMinute, InvalidReason::LimitOutOfRange)),
        "a zero rate would refuse every connector call; disabling connectors is the way to stop them"
    );
    assert_eq!(
        check(&with(LimitKey::MaxLifetimeSeconds, 59), 16),
        Err((LimitKey::MaxLifetimeSeconds, InvalidReason::LimitOutOfRange))
    );
    // per_user above slots is an order refusal, whichever of the two was changed.
    assert_eq!(
        check(&with(LimitKey::PerUser, 9), 16),
        Err((LimitKey::PerUser, InvalidReason::LimitOrder))
    );
    assert_eq!(
        check(&with(LimitKey::Slots, 3), 16),
        Err((LimitKey::PerUser, InvalidReason::LimitOrder))
    );
    assert_eq!(
        check(&with(LimitKey::PerUser, 8), 16),
        Ok(()),
        "per_user may equal slots"
    );
}

#[test]
fn gw_u_035_values_are_read_and_replaced_by_key() {
    let limits = file();
    for key in LimitKey::ALL {
        let changed = limits.with(key, 3);
        assert_eq!(changed.get(key), 3, "{key:?}");
        for other in LimitKey::ALL.into_iter().filter(|other| *other != key) {
            assert_eq!(
                changed.get(other),
                limits.get(other),
                "{key:?} leaves {other:?}"
            );
        }
    }
    assert_eq!(limits.get(LimitKey::IdleSeconds), 600);
}

#[test]
fn gw_u_035_a_file_edit_that_invalidates_a_value_clamps_it_to_the_new_bound() {
    // The file lowered max_live to 4 below an administrator's slots of 8; per_user 4 still fits.
    let (clamped, changes) = clamp(file(), 4);
    assert_eq!(clamped.slots, 4);
    assert_eq!(
        changes,
        vec![Clamped {
            key: LimitKey::Slots,
            from: 8,
            to: 4
        }]
    );
    // Lowering slots below per_user clamps per_user too, in order.
    let (clamped, changes) = clamp(file(), 2);
    assert_eq!((clamped.slots, clamped.per_user), (2, 2));
    assert_eq!(
        changes,
        vec![
            Clamped {
                key: LimitKey::Slots,
                from: 8,
                to: 2
            },
            Clamped {
                key: LimitKey::PerUser,
                from: 4,
                to: 2
            }
        ]
    );
    let (unchanged, changes) = clamp(file(), 16);
    assert_eq!(
        (unchanged, changes),
        (file(), vec![]),
        "a valid set is left alone"
    );
    // A value below its minimum rises to it (a stored zero cannot pass the API, but a seed could change bounds).
    let (raised, changes) = clamp(file().with(LimitKey::MaxLifetimeSeconds, 10), 16);
    assert_eq!(raised.max_lifetime_seconds, 60);
    assert_eq!(
        changes,
        vec![Clamped {
            key: LimitKey::MaxLifetimeSeconds,
            from: 10,
            to: 60
        }]
    );
}

#[test]
fn gw_u_035_admin_writes_meet_the_files_bounds() {
    use super::{admin_bounds, check_write};
    // The admission, sandbox and connector limits have no extra floor; the team limits' own are GW-U-131's.
    for key in LimitKey::ALL.into_iter().filter(|key| !key.is_team()) {
        assert_eq!(
            admin_bounds(key, &file(), 16),
            bounds(key, &file(), 16),
            "{key:?} has no extra floor"
        );
    }
    assert_eq!(
        check_write(&file().with(LimitKey::PerUser, 9), LimitKey::PerUser, 16),
        Err((LimitKey::PerUser, InvalidReason::LimitOrder))
    );
    assert_eq!(
        check_write(
            &file().with(LimitKey::IdleSeconds, 0),
            LimitKey::IdleSeconds,
            16
        ),
        Err((LimitKey::IdleSeconds, InvalidReason::LimitOutOfRange))
    );
}

// TEST: GW-U-131
#[test]
fn gw_u_131_the_team_limits_count_helpers_tokens_and_minutes_and_apply_from_the_next_one() {
    let expected = [
        (LimitKey::TeamMaxMembers, "helpers", "next_member"),
        (LimitKey::TeamTokenBudget, "tokens", "next_team"),
        (LimitKey::TeamDeadlineMinutes, "minutes", "next_team"),
        (LimitKey::MemberIdleMinutes, "minutes", "next_check"),
        (LimitKey::MemberStuckMinutes, "minutes", "next_check"),
        (LimitKey::TeamIdleStopMinutes, "minutes", "next_check"),
    ];
    for (key, unit, applies) in expected {
        assert_eq!((key.unit(), key.applies()), (unit, applies), "{key:?}");
        assert!(key.is_team(), "{key:?}");
    }
    assert!(!LimitKey::Slots.is_team());
}

// TEST: GW-U-131
#[test]
fn gw_u_131_an_administrator_s_team_limits_have_the_contract_s_bounds() {
    let limits = file();
    let expected = [
        // 4 slots... here 8 slots and a per-user cap of 4: three helpers beside the lead.
        (LimitKey::TeamMaxMembers, 1, 3),
        (LimitKey::TeamTokenBudget, 10_000, 100_000_000),
        (LimitKey::TeamDeadlineMinutes, 5, 1_440),
        (LimitKey::MemberIdleMinutes, 1, 1_440),
        (LimitKey::MemberStuckMinutes, 1, 1_440),
        (LimitKey::TeamIdleStopMinutes, 1, 1_440),
    ];
    for (key, min, max) in expected {
        let found = admin_bounds(key, &limits, 16);
        assert_eq!((found.min, found.max), (min, max), "{key:?}");
    }
    // The file's own bounds are wider where the file already allowed more (a token budget of 1, a 1-minute deadline):
    // a file value below an administrator's floor is kept and marked, never refused at load.
    assert_eq!(bounds(LimitKey::TeamTokenBudget, &limits, 16).min, 1);
    assert_eq!(bounds(LimitKey::TeamDeadlineMinutes, &limits, 16).min, 1);
    assert_eq!(bounds(LimitKey::TeamMaxMembers, &limits, 16).max, 8);
}

// TEST: GW-U-131
#[test]
fn gw_u_131_max_members_is_refused_above_the_slots_or_the_per_user_cap_less_one() {
    let with = |key: LimitKey, value: u64| file().with(key, value);
    assert_eq!(
        check_write(
            &with(LimitKey::TeamMaxMembers, 3),
            LimitKey::TeamMaxMembers,
            16
        ),
        Ok(())
    );
    assert_eq!(
        check_write(
            &with(LimitKey::TeamMaxMembers, 4),
            LimitKey::TeamMaxMembers,
            16
        ),
        Err((LimitKey::TeamMaxMembers, InvalidReason::LimitOrder)),
        "a per-user cap of 4 leaves room for 3 helpers beside the lead"
    );
    let slots_three = file().with(LimitKey::Slots, 3).with(LimitKey::PerUser, 3);
    assert_eq!(
        check_write(
            &slots_three.with(LimitKey::TeamMaxMembers, 3),
            LimitKey::TeamMaxMembers,
            16
        ),
        Err((LimitKey::TeamMaxMembers, InvalidReason::LimitOrder))
    );
    assert_eq!(
        check_write(
            &with(LimitKey::TeamMaxMembers, 9),
            LimitKey::TeamMaxMembers,
            64
        ),
        Err((LimitKey::TeamMaxMembers, InvalidReason::LimitOutOfRange))
    );
    // Lowering the slots is never refused because of the helper cap: the effective cap drops from the next helper.
    let lowered = file().with(LimitKey::Slots, 2).with(LimitKey::PerUser, 2);
    assert_eq!(check_write(&lowered, LimitKey::Slots, 16), Ok(()));
    let (clamped, changes) = clamp(lowered, 16);
    assert_eq!((clamped.team_max_members, changes), (3, vec![]));
    // Below an administrator's floor.
    assert_eq!(
        check_write(
            &with(LimitKey::TeamTokenBudget, 9_999),
            LimitKey::TeamTokenBudget,
            16
        ),
        Err((LimitKey::TeamTokenBudget, InvalidReason::LimitOutOfRange))
    );
    assert_eq!(
        check_write(
            &with(LimitKey::TeamDeadlineMinutes, 4),
            LimitKey::TeamDeadlineMinutes,
            16
        ),
        Err((
            LimitKey::TeamDeadlineMinutes,
            InvalidReason::LimitOutOfRange
        ))
    );
    assert_eq!(
        check_write(
            &with(LimitKey::MemberStuckMinutes, 1_441),
            LimitKey::MemberStuckMinutes,
            16
        ),
        Err((LimitKey::MemberStuckMinutes, InvalidReason::LimitOutOfRange))
    );
}

// TEST: GW-U-146
#[test]
fn gw_u_146_the_desktop_limits_count_sandboxes_and_days_within_their_bounds() {
    assert_eq!(
        (
            LimitKey::DesktopMaxSessions.unit(),
            LimitKey::DesktopMaxSessions.applies()
        ),
        ("sandboxes", "next_admission")
    );
    assert_eq!(
        (
            LimitKey::DesktopScreenshotRetentionDays.unit(),
            LimitKey::DesktopScreenshotRetentionDays.applies()
        ),
        ("days", "next_purge")
    );
    assert!(!LimitKey::DesktopMaxSessions.is_team());
    let limits = file();
    // At most the slots (8 here), as the per-user cap; a year of screenshots at most.
    for (key, min, max) in [
        (LimitKey::DesktopMaxSessions, 1, 8),
        (LimitKey::DesktopScreenshotRetentionDays, 1, 365),
    ] {
        let found = bounds(key, &limits, 16);
        assert_eq!((found.min, found.max), (min, max), "{key:?}");
        assert_eq!(admin_bounds(key, &limits, 16), found, "{key:?}");
    }
    // Above the slots is an order refusal, as per_user's is; lowering the slots clamps it at load.
    assert_eq!(
        check(&limits.with(LimitKey::DesktopMaxSessions, 9), 16),
        Err((LimitKey::DesktopMaxSessions, InvalidReason::LimitOrder))
    );
    assert_eq!(
        check(
            &limits.with(LimitKey::DesktopScreenshotRetentionDays, 366),
            16
        ),
        Err((
            LimitKey::DesktopScreenshotRetentionDays,
            InvalidReason::LimitOutOfRange
        ))
    );
    let (clamped, changes) = clamp(
        limits.with(LimitKey::Slots, 1).with(LimitKey::PerUser, 1),
        16,
    );
    assert_eq!(clamped.desktop_max_sessions, 1);
    assert!(
        changes
            .iter()
            .any(|change| change.key == LimitKey::DesktopMaxSessions)
    );
}

// TEST: GW-U-152
#[test]
fn gw_u_152_the_computer_caps_count_screenshots_and_actions_from_the_next_session() {
    for (key, unit, max) in [
        (LimitKey::ComputerMaxScreenshotsPerRun, "screenshots", 1_000),
        (LimitKey::ComputerMaxActionsPerRun, "actions", 5_000),
    ] {
        assert_eq!(
            (key.unit(), key.applies()),
            (unit, "next_session"),
            "{key:?}"
        );
        assert!(!key.is_team());
        let limits = file();
        let found = bounds(key, &limits, 16);
        assert_eq!((found.min, found.max), (1, max), "{key:?}");
        assert_eq!(admin_bounds(key, &limits, 16), found, "{key:?}");
        assert_eq!(check(&limits.with(key, max), 16), Ok(()), "{key:?}");
        for refused in [0, max + 1] {
            assert_eq!(
                check(&limits.with(key, refused), 16),
                Err((key, InvalidReason::LimitOutOfRange)),
                "{key:?} {refused}"
            );
        }
    }
}
