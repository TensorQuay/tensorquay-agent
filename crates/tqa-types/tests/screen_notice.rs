//! PROTO-U-038: `screen.notice` is the gateway's browser-only screen notice (ADR-0039's contract): what happened, who,
//! when and the event it came after; a hand-back also says how long and how it ended. It round-trips exactly, carries
//! no sequence, and is refused malformed, naming why.
use serde_json::{Value, json};
use tqa_types::{ErrorCode, ScreenEnded, ScreenNoticeKind, ScreenPerson, ServerFrame};

const PERSON: &str = "0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6c";

fn notice(kind: &str) -> Value {
    json!({"type": "screen.notice", "kind": kind, "by": {"id": PERSON, "name": "Ada"},
        "at": "2026-10-05T12:00:00Z", "after_seq": 41})
}

// TEST: PROTO-U-038
#[test]
fn proto_u_038_a_screen_notice_round_trips_without_a_sequence()
-> Result<(), Box<dyn std::error::Error>> {
    let mut wire = notice("screen.handed_back");
    wire["seconds"] = json!(95);
    wire["ended"] = json!("socket_closed");
    let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
    assert_eq!(
        frame,
        ServerFrame::ScreenNotice {
            kind: ScreenNoticeKind::HandedBack,
            by: ScreenPerson {
                id: PERSON.to_owned(),
                name: "Ada".to_owned()
            },
            at: "2026-10-05T12:00:00Z".to_owned(),
            after_seq: 41,
            seconds: Some(95),
            ended: Some(ScreenEnded::SocketClosed),
            reason: None,
        }
    );
    assert_eq!(
        serde_json::to_value(&frame)?,
        wire,
        "exactly the wire shape: no seq"
    );
    for kind in ["screen.taken_over", "screen.signed_out"] {
        let wire = notice(kind);
        let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
        assert_eq!(serde_json::to_value(&frame)?, wire, "{kind}");
    }
    // Handed back by a restart: no length.
    let mut wire = notice("screen.handed_back");
    wire["ended"] = json!("restart");
    let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
    assert_eq!(serde_json::to_value(&frame)?, wire);
    Ok(())
}

// TEST: PROTO-U-038
#[test]
fn proto_u_038_a_malformed_screen_notice_is_refused() {
    let with = |kind: &str, change: &dyn Fn(&mut Value)| {
        let mut wire = notice(kind);
        change(&mut wire);
        wire
    };
    let hand_back = |wire: &mut Value| {
        wire["seconds"] = json!(95);
        wire["ended"] = json!("hand_back");
    };
    for wire in [
        with("screen.typed", &|_| {}),
        with("screen.taken_over", &|wire| {
            wire["by"]["id"] = json!("not-a-uuid");
        }),
        with("screen.taken_over", &|wire| wire["by"]["name"] = json!("")),
        with("screen.taken_over", &|wire| {
            wire["by"]["name"] = json!("Ada\nLovelace");
        }),
        with("screen.taken_over", &|wire| {
            wire["by"]["name"] = json!("x".repeat(255));
        }),
        with("screen.taken_over", &|wire| wire["at"] = json!("yesterday")),
        with("screen.taken_over", &|wire| wire["after_seq"] = json!(-1)),
        with("screen.taken_over", &|wire| {
            wire["by"]["typed"] = json!("secret");
        }),
        // A hand-back without how long or how it ended, and either on another kind.
        with("screen.handed_back", &|_| {}),
        with("screen.handed_back", &|wire| wire["seconds"] = json!(95)),
        with("screen.taken_over", &hand_back),
        with("screen.signed_out", &|wire| {
            wire["ended"] = json!("hand_back");
        }),
        with("screen.handed_back", &|wire| {
            hand_back(wire);
            wire["ended"] = json!("timeout");
        }),
        // A restart's hand-back has no length; any other has one.
        with("screen.handed_back", &|wire| {
            hand_back(wire);
            wire["ended"] = json!("restart");
        }),
        with("screen.handed_back", &|wire| {
            wire["ended"] = json!("socket_closed");
        }),
    ] {
        let refused = ServerFrame::parse(wire.to_string().as_bytes());
        assert!(
            matches!(&refused, Err(error) if error.code == ErrorCode::InvalidFrame),
            "{wire}: {refused:?}"
        );
    }
}

// TEST: PROTO-U-039
#[test]
fn proto_u_039_a_failed_sign_out_says_why_and_only_it_does()
-> Result<(), Box<dyn std::error::Error>> {
    for reason in ["not_cleared", "unconfirmed"] {
        let mut wire = notice("screen.sign_out_failed");
        wire["reason"] = json!(reason);
        let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
        assert_eq!(serde_json::to_value(&frame)?, wire, "{reason}");
    }
    let refused = |wire: Value| matches!(ServerFrame::parse(wire.to_string().as_bytes()), Err(error) if error.code == ErrorCode::InvalidFrame);
    assert!(
        refused(notice("screen.sign_out_failed")),
        "a failure without its reason"
    );
    let mut signed_out = notice("screen.signed_out");
    signed_out["reason"] = json!("unconfirmed");
    assert!(refused(signed_out), "a reason on another kind");
    let mut unknown = notice("screen.sign_out_failed");
    unknown["reason"] = json!("timeout");
    assert!(refused(unknown), "a reason outside the list");
    Ok(())
}

// TEST: PROTO-U-039
#[test]
fn proto_u_039_the_runtime_answers_a_sign_out_with_ok_or_a_reason()
-> Result<(), Box<dyn std::error::Error>> {
    use tqa_types::{ClientFrame, SignOutFailure};
    let ask = ClientFrame::parse(json!({"type": "screen.sign_out"}).to_string().as_bytes())?;
    assert_eq!(ask, ClientFrame::ScreenSignOut {});
    assert_eq!(
        serde_json::to_value(&ask)?,
        json!({"type": "screen.sign_out"})
    );
    let done = ServerFrame::parse(
        json!({"type": "screen.sign_out.done", "ok": true})
            .to_string()
            .as_bytes(),
    )?;
    assert_eq!(
        done,
        ServerFrame::ScreenSignOutDone {
            ok: true,
            reason: None,
            ready: true
        }
    );
    for (wire, reason) in [
        ("profile_in_use", SignOutFailure::ProfileInUse),
        ("io_error", SignOutFailure::IoError),
    ] {
        let frame = ServerFrame::parse(
            json!({"type": "screen.sign_out.done", "ok": false, "reason": wire})
                .to_string()
                .as_bytes(),
        )?;
        assert_eq!(
            frame,
            ServerFrame::ScreenSignOutDone {
                ok: false,
                reason: Some(reason),
                ready: true
            }
        );
    }
    for wire in [
        json!({"type": "screen.sign_out.done", "ok": false}),
        json!({"type": "screen.sign_out.done", "ok": true, "reason": "io_error"}),
        json!({"type": "screen.sign_out.done", "ok": false, "reason": "disk_full"}),
        json!({"type": "screen.sign_out.done"}),
    ] {
        assert!(
            matches!(ServerFrame::parse(wire.to_string().as_bytes()), Err(error) if error.code == ErrorCode::InvalidFrame),
            "{wire}"
        );
    }
    Ok(())
}

// TEST: PROTO-U-041
#[test]
fn proto_u_041_the_runtime_says_when_a_run_end_reset_is_over()
-> Result<(), Box<dyn std::error::Error>> {
    for ok in [true, false] {
        let wire = json!({"type": "screen.reset", "ok": ok});
        let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
        assert_eq!(frame, ServerFrame::ScreenReset { ok, ready: true });
        assert_eq!(serde_json::to_value(&frame)?, wire);
    }
    // The browser didn't come back in time (16r's sec_059_a): said, and carried both ways.
    let wire = json!({"type": "screen.reset", "ok": true, "ready": false});
    let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
    assert_eq!(
        frame,
        ServerFrame::ScreenReset {
            ok: true,
            ready: false
        }
    );
    assert_eq!(serde_json::to_value(&frame)?, wire);
    for bad in [
        json!({"type": "screen.reset"}),
        json!({"type": "screen.reset", "ok": "yes"}),
    ] {
        assert!(
            ServerFrame::parse(bad.to_string().as_bytes()).is_err(),
            "{bad}"
        );
    }
    Ok(())
}
