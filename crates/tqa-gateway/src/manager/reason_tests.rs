//! GW-U-167: the reasons a sandbox's create and teardown log (GW-I-259) are a vocabulary the logs are read by (the
//! rerun of E2E-AGENTS-020's try 8): each listed value is pinned here. The match below only makes a new reason need
//! an arm, a reminder to add its row beside it; only the row pins the value.
use super::{Create, Teardown};

/// Every teardown reason with its logged value. The match fails to compile when a reason has no arm (not no row):
/// adding the arm is the reminder to add the row.
fn teardowns() -> Vec<(Teardown, &'static str)> {
    let rows = vec![
        (Teardown::HandOver, "hand_over"),
        (Teardown::Eviction, "eviction"),
        (Teardown::EnvironmentSwitch, "environment_switch"),
        (Teardown::WorkspaceDeleted, "workspace_deleted"),
        (Teardown::TeamStopped, "team_stopped"),
        (Teardown::Idle, "idle"),
        (Teardown::Lifetime, "lifetime"),
        (Teardown::LifetimeBusy, "lifetime_busy"),
        (Teardown::Unconfirmed, "unconfirmed"),
        (Teardown::Refused, "refused"),
        (Teardown::Quota, "quota"),
        (Teardown::StartFailed, "start_failed"),
        (Teardown::TaskDeleted, "task_deleted"),
        (Teardown::RepeatedLoss, "repeated_loss"),
        (Teardown::Retry, "retry"),
        (Teardown::Reconcile, "reconcile"),
        (Teardown::Lost, "lost"),
        (Teardown::Stray, "stray"),
    ];
    for (reason, _) in &rows {
        match reason {
            Teardown::HandOver
            | Teardown::Eviction
            | Teardown::EnvironmentSwitch
            | Teardown::WorkspaceDeleted
            | Teardown::TeamStopped
            | Teardown::Idle
            | Teardown::Lifetime
            | Teardown::LifetimeBusy
            | Teardown::Unconfirmed
            | Teardown::Refused
            | Teardown::Quota
            | Teardown::StartFailed
            | Teardown::TaskDeleted
            | Teardown::RepeatedLoss
            | Teardown::Retry
            | Teardown::Reconcile
            | Teardown::Lost
            | Teardown::Stray => {}
        }
    }
    rows
}

fn creates() -> Vec<(Create, &'static str)> {
    let rows = vec![
        (Create::Work, "work"),
        (Create::Replacement, "replacement"),
        (Create::StartAgain, "start_again"),
    ];
    for (reason, _) in &rows {
        match reason {
            Create::Work | Create::Replacement | Create::StartAgain => {}
        }
    }
    rows
}

// TEST: GW-U-167
#[test]
fn gw_u_167_each_reason_logs_its_own_fixed_value() {
    let teardowns = teardowns();
    let creates = creates();
    assert_eq!(teardowns.len(), 18);
    assert_eq!(creates.len(), 3);
    let mut seen = std::collections::HashSet::new();
    for (reason, value) in &teardowns {
        assert_eq!(reason.as_str(), *value, "{reason:?}");
        assert!(seen.insert(*value), "{value} twice among the teardowns");
    }
    let mut seen = std::collections::HashSet::new();
    for (reason, value) in &creates {
        assert_eq!(reason.as_str(), *value, "{reason:?}");
        assert!(seen.insert(*value), "{value} twice among the creates");
    }
    for (_, value) in teardowns
        .iter()
        .map(|(r, v)| (r.as_str(), *v))
        .chain(creates.iter().map(|(r, v)| (r.as_str(), *v)))
    {
        assert!(
            !value.is_empty() && value.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{value}: lowercase words joined by _"
        );
    }
}
