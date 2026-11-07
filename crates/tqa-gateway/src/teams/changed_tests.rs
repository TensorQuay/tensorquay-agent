//! GW-U-136: the change marks a NOTIFY payload makes, and a team's version.
use super::Changes;
use uuid::Uuid;

// TEST: GW-U-136
#[test]
fn gw_u_136_payloads_mark_a_team_or_a_channel_and_nothing_else() {
    let changes = Changes::default();
    let (team, channel) = (Uuid::now_v7(), Uuid::now_v7());
    changes.mark(&format!("t:{team}"));
    changes.mark(&format!("c:{channel}"));
    changes.mark("x:whatever");
    changes.mark("t:not-a-uuid");
    let marked = changes.take();
    assert_eq!(marked.teams.into_iter().collect::<Vec<_>>(), [team]);
    assert_eq!(marked.channels.into_iter().collect::<Vec<_>>(), [channel]);
    assert!(!marked.all);
    assert!(changes.take().is_empty(), "a flush takes the marks");
    changes.mark_all();
    assert!(changes.take().all, "a (re)connect marks every open team");
}

// TEST: GW-U-136
#[test]
fn gw_u_136_a_version_starts_at_the_gateways_start_and_only_rises() {
    let changes = Changes::default();
    let team = Uuid::now_v7();
    let first = changes.version(team);
    assert!(
        first > 1_700_000_000_000,
        "starts at the start time in milliseconds: {first}"
    );
    assert_eq!(changes.bump(team), first + 1);
    assert_eq!(changes.bump(team), first + 2);
    assert_eq!(changes.version(team), first + 2);
    // A later gateway starts above any version this one could have reached in practice.
    let later = Changes {
        base: first + 1_000,
        ..Changes::default()
    };
    assert!(later.version(team) > changes.version(team));
}
