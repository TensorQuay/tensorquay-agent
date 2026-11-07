//! PROTO-U-034: `team.changed` is the gateway's browser-only notice that an agent team changed (ADR-0034, the slice
//! S6 contract): the team's id, its version and its latest message's seq. It round-trips exactly, carries no
//! sequence, and is refused malformed, naming the field.
use serde_json::json;
use tqa_types::{ErrorCode, ServerFrame};

const TEAM: &str = "0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6b";

#[test]
fn proto_u_034_team_changed_round_trips_without_a_sequence()
-> Result<(), Box<dyn std::error::Error>> {
    let wire = json!({"type": "team.changed", "team": TEAM, "version": 9, "last_seq": 15});
    let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
    assert_eq!(
        frame,
        ServerFrame::TeamChanged {
            team: TEAM.to_owned(),
            version: 9,
            last_seq: 15,
        }
    );
    assert_eq!(
        serde_json::to_value(&frame)?,
        wire,
        "exactly the wire shape: no seq"
    );
    // A team that has no messages yet.
    let empty = json!({"type": "team.changed", "team": TEAM, "version": 1, "last_seq": 0});
    assert!(ServerFrame::parse(empty.to_string().as_bytes()).is_ok());
    Ok(())
}

#[test]
fn proto_u_034_a_malformed_team_changed_is_refused() -> Result<(), Box<dyn std::error::Error>> {
    for wire in [
        json!({"type": "team.changed", "version": 1, "last_seq": 0}),
        json!({"type": "team.changed", "team": TEAM, "last_seq": 0}),
        json!({"type": "team.changed", "team": TEAM, "version": 1}),
        json!({"type": "team.changed", "team": TEAM, "version": -1, "last_seq": 0}),
        json!({"type": "team.changed", "team": TEAM, "version": 1, "last_seq": "15"}),
    ] {
        let error = ServerFrame::parse(wire.to_string().as_bytes())
            .err()
            .ok_or_else(|| format!("{wire} accepted"))?;
        assert_eq!(error.code, ErrorCode::InvalidFrame, "{wire}: {error:?}");
    }
    // The team's id as the gateway writes it, or the frame is refused naming the field.
    for team in [TEAM.to_uppercase(), "not-a-team".to_owned(), String::new()] {
        let wire = json!({"type": "team.changed", "team": team, "version": 1, "last_seq": 0});
        let error = ServerFrame::parse(wire.to_string().as_bytes())
            .err()
            .ok_or_else(|| format!("{wire} accepted"))?;
        assert_eq!(
            (error.code, error.path.as_str()),
            (ErrorCode::InvalidFrame, "team"),
            "{wire}"
        );
    }
    // An additive field is accepted, as on every server frame.
    let later = json!({"type": "team.changed", "team": TEAM, "version": 2, "last_seq": 3, "reason": "helper"});
    assert!(ServerFrame::parse(later.to_string().as_bytes()).is_ok());
    Ok(())
}
