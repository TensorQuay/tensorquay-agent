//! PROTO-U-045 (ADR-0039 section 6, amended for 18.0; PRD item 24): a desktop sign-in kept for a whole task. The
//! gateway tells the runtime the task's switch on `session.config` (`keep_sign_in`) and, when it changes, with
//! `screen.keep_sign_in`; the runtime's `screen.reset` says when a run's end kept the profile (`kept`); the gateway's
//! notices say who turned the switch on or off.
use serde_json::{Value, json};
use tqa_types::{
    ClientFrame, PROTOCOL_REVISION, PROTOCOL_REVISIONS, ScreenNoticeKind, ScreenPerson, ServerFrame,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PERSON: &str = "0192b3a4-5c6d-7e8f-9a0b-1c2d3e4f5a6c";

fn session_config() -> Value {
    json!({"type":"session.config","conversation_id":"conversation","last_stored_seq":0,
    "route":{"name":"local","protocol":"openai_chat","model":"fake-model","relay_path":"/relay/r/local","context_window":32768,"max_output":4096,"native_tools":true,"parallel_tools":true,"vision":false,"reasoning":null},
    "network":{"allowed":[],"more":0},"limits":{"max_steps":10,"wall_clock_secs":600,"tool_result_bytes":8192},"workspace":{"display":"/workspace"},"protocol":1})
}

// TEST: PROTO-U-045
#[test]
fn proto_u_045_the_switch_rides_on_session_config_and_is_absent_when_unknown() -> TestResult {
    let plain = ClientFrame::parse(session_config().to_string().as_bytes())?;
    let ClientFrame::SessionConfig { keep_sign_in, .. } = &plain else {
        return Err("not a session.config".into());
    };
    assert_eq!(*keep_sign_in, None, "an older gateway says nothing");
    assert!(
        serde_json::to_value(&plain)?.get("keep_sign_in").is_none(),
        "nothing said, nothing written"
    );
    for on in [true, false] {
        let mut wire = session_config();
        wire["keep_sign_in"] = json!(on);
        let frame = ClientFrame::parse(wire.to_string().as_bytes())?;
        let ClientFrame::SessionConfig { keep_sign_in, .. } = &frame else {
            return Err("not a session.config".into());
        };
        assert_eq!(*keep_sign_in, Some(on));
        assert_eq!(serde_json::to_value(&frame)?["keep_sign_in"], json!(on));
    }
    let mut bad = session_config();
    bad["keep_sign_in"] = json!("yes");
    assert!(ClientFrame::parse(bad.to_string().as_bytes()).is_err());
    Ok(())
}

#[test]
fn proto_u_045_the_gateway_tells_the_runtime_when_the_switch_changes() -> TestResult {
    for on in [true, false] {
        let wire = json!({"type": "screen.keep_sign_in", "on": on});
        let frame = ClientFrame::parse(wire.to_string().as_bytes())?;
        assert_eq!(frame, ClientFrame::ScreenKeepSignIn { on });
        assert_eq!(
            serde_json::to_value(&frame)?,
            wire,
            "exactly the wire shape"
        );
    }
    for bad in [
        json!({"type": "screen.keep_sign_in"}),
        json!({"type": "screen.keep_sign_in", "on": "yes"}),
        json!({"type": "screen.keep_sign_in", "on": true, "who": "me"}),
    ] {
        assert!(
            ClientFrame::parse(bad.to_string().as_bytes()).is_err(),
            "{bad}"
        );
    }
    Ok(())
}

#[test]
fn proto_u_045_a_reset_says_when_the_run_end_kept_the_sign_in() -> TestResult {
    // As before: a wipe, nothing said of keeping (an older runtime's frame).
    let wire = json!({"type": "screen.reset", "ok": true});
    let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
    assert_eq!(
        frame,
        ServerFrame::ScreenReset {
            ok: true,
            ready: true,
            kept: false
        }
    );
    assert_eq!(serde_json::to_value(&frame)?, wire, "false is not written");
    let wire = json!({"type": "screen.reset", "ok": true, "kept": true});
    let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
    assert_eq!(
        frame,
        ServerFrame::ScreenReset {
            ok: true,
            ready: true,
            kept: true
        }
    );
    assert_eq!(serde_json::to_value(&frame)?, wire);
    // A kept profile is the one the run ended with: `ok` false says a wipe failed, which keeping never does.
    let bad = json!({"type": "screen.reset", "ok": false, "kept": true});
    assert!(ServerFrame::parse(bad.to_string().as_bytes()).is_err());
    Ok(())
}

#[test]
fn proto_u_045_the_notices_say_who_turned_the_switch_on_or_off() -> TestResult {
    for (kind, name) in [
        (ScreenNoticeKind::SignInKept, "screen.sign_in_kept"),
        (ScreenNoticeKind::SignInReleased, "screen.sign_in_released"),
    ] {
        let wire = json!({"type": "screen.notice", "kind": name, "by": {"id": PERSON, "name": "Ada"},
            "at": "2026-11-15T12:00:00Z", "after_seq": 41});
        let frame = ServerFrame::parse(wire.to_string().as_bytes())?;
        assert_eq!(
            frame,
            ServerFrame::ScreenNotice {
                kind,
                by: Some(ScreenPerson {
                    id: PERSON.to_owned(),
                    name: "Ada".to_owned()
                }),
                at: "2026-11-15T12:00:00Z".to_owned(),
                after_seq: 41,
                seconds: None,
                ended: None,
                reason: None,
            }
        );
        assert_eq!(serde_json::to_value(&frame)?, wire);
        // Neither carries a hand-back's fields or a failed sign-out's reason.
        for (field, value) in [
            ("seconds", json!(3)),
            ("ended", json!("hand_back")),
            ("reason", json!("not_cleared")),
        ] {
            let mut bad = wire.clone();
            bad[field] = value;
            assert!(
                ServerFrame::parse(bad.to_string().as_bytes()).is_err(),
                "{bad}"
            );
        }
    }
    Ok(())
}

#[test]
fn proto_u_045_revision_15_names_the_switch_and_the_floor_stays_10() {
    assert!(PROTOCOL_REVISION >= 15, "revision 15 stays in the table");
    assert_eq!(tqa_types::MIN_RUNTIME_REVISION, 10);
    assert_eq!(tqa_types::KEEP_SIGN_IN_REVISION, 15);
    let row = &PROTOCOL_REVISIONS[14];
    assert_eq!(row.number, 15);
    for named in [
        ("ClientFrame", "keep_sign_in"),
        ("ClientFrame", "screen.keep_sign_in"),
        ("ServerFrame", "kept"),
        ("ScreenNoticeKind", "screen.sign_in_kept"),
        ("ScreenNoticeKind", "screen.sign_in_released"),
    ] {
        assert!(row.schema.contains(&named), "{named:?} in revision 15");
    }
}
