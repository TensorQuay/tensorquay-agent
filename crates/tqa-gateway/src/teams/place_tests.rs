//! GW-U-135: `Conversation.team` as the S6 contract shapes it, for the task's own person and for anyone else.
use super::{Attention, Lead, TeamPlace, Title, attention, can_stop, helper_state, slot_place};
use serde_json::json;
use uuid::Uuid;

fn ids() -> (Uuid, Uuid) {
    (
        Uuid::parse_str("0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6b").unwrap_or_default(),
        Uuid::parse_str("0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6c").unwrap_or_default(),
    )
}

fn lead() -> TeamPlace {
    TeamPlace::Lead {
        id: ids().0,
        state: "running".into(),
        helpers: 3,
        attention: Attention {
            stuck: 1,
            failed: 0,
        },
        waiting_for_you: Some(true),
    }
}

fn member() -> TeamPlace {
    TeamPlace::Member {
        id: ids().0,
        state: "running".into(),
        lead: Lead {
            id: ids().1,
            title: Some(Title {
                title: Some("Report on Q3".into()),
            }),
        },
        name: Some("researcher".into()),
        helper_state: Some("working"),
        can_stop: Some(true),
    }
}

// TEST: GW-U-135
#[test]
fn gw_u_135_the_own_person_gets_every_field() -> Result<(), serde_json::Error> {
    let (team, task) = ids();
    assert_eq!(
        serde_json::to_value(lead().for_viewer(true))?,
        json!({"role": "lead", "id": team, "state": "running", "helpers": 3,
               "attention": {"stuck": 1, "failed": 0}, "waiting_for_you": true})
    );
    assert_eq!(
        serde_json::to_value(member().for_viewer(true))?,
        json!({"role": "member", "id": team, "state": "running", "lead": {"id": task, "title": "Report on Q3"},
               "name": "researcher", "helper_state": "working", "can_stop": true})
    );
    // An untitled lead is null, not left out.
    let mut untitled = member();
    if let TeamPlace::Member { lead, .. } = &mut untitled {
        lead.title = Some(Title { title: None });
    }
    assert_eq!(
        serde_json::to_value(untitled)?["lead"],
        json!({"id": task, "title": null})
    );
    Ok(())
}

// TEST: GW-U-135
#[test]
fn gw_u_135_anyone_else_gets_no_content() -> Result<(), serde_json::Error> {
    let (team, task) = ids();
    // SEC-005: counts and ids only; never a name, the lead's title, a helper's state or a question.
    assert_eq!(
        serde_json::to_value(lead().for_viewer(false))?,
        json!({"role": "lead", "id": team, "state": "running", "helpers": 3,
               "attention": {"stuck": 1, "failed": 0}})
    );
    assert_eq!(
        serde_json::to_value(member().for_viewer(false))?,
        json!({"role": "member", "id": team, "state": "running", "lead": {"id": task}})
    );
    Ok(())
}

// TEST: GW-U-135
#[test]
fn gw_u_135_stored_helper_states_read_as_the_contracts() {
    for (stored, shown) in [
        ("starting", "working"),
        ("working", "working"),
        ("waiting", "waiting_lead"),
        ("queued", "queued"),
        ("stuck", "stuck"),
        ("done", "done"),
        ("stopped", "stopped"),
        ("failed", "failed"),
        ("something new", "unknown"),
    ] {
        assert_eq!(helper_state(stored), shown, "{stored}");
    }
}

// TEST: GW-U-168
#[test]
fn gw_u_168_stop_is_offered_for_what_stop_member_would_stop() {
    for (stored, failed_run, stoppable) in [
        ("starting", false, true),
        ("queued", false, true),
        ("working", false, true),
        ("waiting", false, true),
        ("stuck", false, true),
        ("done", false, true),
        // A helper whose run failed keeps its seat until stopped; one that couldn't start has nothing to stop.
        ("failed", true, true),
        ("failed", false, false),
        ("stopped", false, false),
    ] {
        assert_eq!(
            can_stop(true, stored, failed_run),
            stoppable,
            "{stored} {failed_run}"
        );
        assert!(
            !can_stop(false, stored, failed_run),
            "a team that isn't running stops no one: {stored}"
        );
    }
}

// TEST: GW-U-169
/// A running team's lead shows its stuck and failed helpers; a stopping or ended one shows none, so the task list's red
/// means something needs the person now.
#[test]
fn gw_u_169_an_ended_teams_lead_counts_no_stuck_or_failed_helper() {
    assert_eq!(
        attention(true, 1, 2),
        Attention {
            stuck: 1,
            failed: 2
        }
    );
    assert_eq!(
        attention(false, 1, 2),
        Attention {
            stuck: 0,
            failed: 0
        }
    );
    assert_eq!(
        attention(false, 0, 0),
        Attention {
            stuck: 0,
            failed: 0
        }
    );
}

// TEST: GW-U-199
// 16n T5 (c16k real-world B10b): a helper on the lead's files waiting for its turn on them read "Waiting for a slot
// (next)". Its place counts from 1 in the slot queue only; a wait for the workspace, its own sandbox or a Desktop has none.
#[test]
fn gw_u_199_a_helper_has_a_place_only_in_the_slot_queue() -> Result<(), tqa_types::FrameError> {
    use tqa_types::{QueuePosition, QueueReason};
    let at = QueuePosition::new;
    assert_eq!(slot_place(at(0)?.with_reason(QueueReason::Slot)), Some(1));
    assert_eq!(slot_place(at(2)?.with_reason(QueueReason::Slot)), Some(3));
    assert_eq!(slot_place(at(0)?.with_reason(QueueReason::Workspace)), None);
    assert_eq!(
        slot_place(at(0)?.with_reason(QueueReason::OwnSandbox)),
        None
    );
    assert_eq!(
        slot_place(at(1)?.with_reason(QueueReason::DesktopSessions)),
        None
    );
    // An older gateway's position without a reason was a slot's.
    assert_eq!(slot_place(at(4)?), Some(5));
    assert_eq!(slot_place(at(tqa_types::MAX_QUEUE_AHEAD)?), Some(10_001));
    Ok(())
}
