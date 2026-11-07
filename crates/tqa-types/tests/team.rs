//! PROTO-U-023: the agent-team wire types (ADR-0034, the slice S1 contract): every stored message type, the read shape,
//! the posts and `start_member`'s brief round-trip exactly; a missing required field and an unknown field are refused
//! naming the field.
use serde_json::{Value, json};
use tqa_types::{
    CheckReport, FilesTaken, NoticeKind, StartMember, TakeFiles, TeamMessage, TeamMessageBody,
    TeamMessageType, TeamPost, WorkspaceMode,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn round_trip<T: serde::de::DeserializeOwned + serde::Serialize>(wire: &Value) -> TestResult {
    let parsed: T = serde_json::from_value(wire.clone())?;
    assert_eq!(&serde_json::to_value(&parsed)?, wire, "{wire}");
    Ok(())
}

fn refusal<T: serde::de::DeserializeOwned + std::fmt::Debug>(
    wire: Value,
) -> Result<String, Box<dyn std::error::Error>> {
    match serde_json::from_value::<T>(wire) {
        Ok(parsed) => Err(format!("accepted {parsed:?}").into()),
        Err(error) => Ok(error.to_string()),
    }
}

fn bodies() -> Vec<Value> {
    vec![
        json!({"type": "assign", "text": "Check the north region."}),
        json!({"type": "assign", "text": "Your brief.", "brief": {"objective": "Find the Q3 revenue.", "output": "A table.",
               "sources": "Only reports/.", "boundaries": "Write only results.md."}}),
        json!({"type": "question", "text": "Which format?"}),
        json!({"type": "answer", "text": "CSV.", "question_seq": 5}),
        json!({"type": "report", "text": "Done.", "files": [{"name": "results.md", "size": 1204, "sha256": "ab".repeat(32)}]}),
        json!({"type": "report", "text": "Done, nothing to hand back.", "files": []}),
        json!({"type": "ask", "text": "May I use last year's figures?"}),
        json!({"type": "decision", "text": "Use CSV.", "question_seq": 5}),
        json!({"type": "decision", "text": "Stop the market part."}),
        json!({"type": "person_question", "text": "Which quarter?"}),
        json!({"type": "person_answer", "text": "Q3.", "question_seq": 9, "by": "Robin"}),
        json!({"type": "person_message", "text": "Use the new template.", "member": "researcher", "by": "Robin"}),
        json!({"type": "notice", "kind": "member_stuck", "member": "researcher", "text": "researcher has had no progress for 15 minutes."}),
        json!({"type": "notice", "kind": "member_queued", "member": "reviewer", "position": 2, "text": "reviewer waits for a slot."}),
        json!({"type": "notice", "kind": "budget_80", "text": "80 % of the budget is used."}),
        json!({"type": "done", "text": "The report is in report.md."}),
        json!({"type": "ended", "reason": "done", "summary": "The report is in report.md.",
               "tokens": {"total": 41200, "members": [{"name": "researcher", "tokens": 18800}]},
               "reports": [{"member": "researcher", "seq": 12}],
               "files": [{"member": "researcher", "seq": 12, "name": "results.md", "size": 1204}],
               "text": "The agent team ended: the lead said it was done."}),
        json!({"type": "ended", "reason": "deadline", "tokens": {"total": 0, "members": []}, "reports": [], "files": [],
               "text": "The agent team ended: its deadline passed."}),
    ]
}

#[test]
fn proto_u_023_every_stored_type_round_trips_exactly() -> TestResult {
    let mut seen = Vec::new();
    for body in bodies() {
        round_trip::<TeamMessageBody>(&body)?;
        let parsed: TeamMessageBody = serde_json::from_value(body.clone())?;
        assert_eq!(json!(parsed.message_type()), body["type"]);
        seen.push(parsed.message_type());
    }
    for kind in TeamMessageType::ALL {
        assert!(seen.contains(&kind), "{kind:?} has no case");
    }
    assert_eq!(TeamMessageType::ALL.len(), 12);
    Ok(())
}

#[test]
fn proto_u_023_the_read_shape_carries_seq_author_addressee_and_body() -> TestResult {
    let wire = json!({"seq": 7, "type": "answer", "from": {"kind": "member", "name": "researcher"},
                      "to": {"kind": "lead"}, "at": "2026-10-02T09:14:03Z", "text": "North: 4.1 M", "question_seq": 5});
    round_trip::<TeamMessage>(&wire)?;
    let message: TeamMessage = serde_json::from_value(wire)?;
    assert_eq!(message.seq, 7);
    assert_eq!(message.body.message_type(), TeamMessageType::Answer);
    for to in [
        json!({"kind": "member", "name": "reviewer"}),
        json!({"kind": "members"}),
        json!({"kind": "team"}),
        json!({"kind": "person"}),
    ] {
        let wire = json!({"seq": 8, "type": "question", "from": {"kind": "lead"}, "to": to, "at": "2026-10-02T09:15:00Z",
                          "text": "Which format?"});
        round_trip::<TeamMessage>(&wire)?;
    }
    let person = json!({"seq": 9, "type": "person_message", "from": {"kind": "person", "name": "Robin"},
                        "to": {"kind": "lead"}, "at": "2026-10-02T09:16:00Z", "text": "Use the template.",
                        "member": "reviewer", "by": "Robin"});
    round_trip::<TeamMessage>(&person)
}

#[test]
fn proto_u_023_a_missing_required_field_is_refused_naming_it() -> TestResult {
    for (wire, field) in [
        (json!({"type": "answer", "text": "CSV."}), "question_seq"),
        (
            json!({"type": "person_answer", "text": "Q3.", "question_seq": 9}),
            "by",
        ),
        (
            json!({"type": "person_message", "text": "Hi.", "by": "Robin"}),
            "member",
        ),
        (json!({"type": "notice", "text": "A notice."}), "kind"),
        (json!({"type": "question"}), "text"),
    ] {
        let message = refusal::<TeamMessageBody>(wire)?;
        assert!(message.contains(field), "{message}");
    }
    Ok(())
}

#[test]
fn proto_u_023_an_unknown_field_or_type_is_refused_naming_it() -> TestResult {
    let message = refusal::<TeamMessageBody>(
        json!({"type": "question", "text": "Which?", "to": "reviewer"}),
    )?;
    assert!(message.contains("`to`"), "{message}");
    let message = refusal::<TeamMessageBody>(json!({"type": "vote", "text": "Yes."}))?;
    assert!(message.contains("vote"), "{message}");
    let message = refusal::<TeamMessageBody>(
        json!({"type": "notice", "kind": "member_bored", "text": "No."}),
    )?;
    assert!(message.contains("member_bored"), "{message}");
    Ok(())
}

#[test]
fn proto_u_023_posts_round_trip_and_only_assign_names_its_addressee() -> TestResult {
    for wire in [
        json!({"type": "assign", "to": "researcher", "text": "Check the north region."}),
        json!({"type": "question", "text": "Which format?"}),
        json!({"type": "decision", "text": "Use CSV.", "question_seq": 5}),
        json!({"type": "person_question", "text": "Which quarter?"}),
        json!({"type": "answer", "question_seq": 5, "text": "CSV."}),
        json!({"type": "report", "text": "Done.", "files": ["results.md", "charts/q3.png"]}),
        json!({"type": "ask", "text": "May I use last year's figures?"}),
    ] {
        round_trip::<TeamPost>(&wire)?;
    }
    let message = refusal::<TeamPost>(json!({"type": "assign", "text": "No addressee."}))?;
    assert!(message.contains("`to`"), "{message}");
    let message =
        refusal::<TeamPost>(json!({"type": "question", "to": "researcher", "text": "Which?"}))?;
    assert!(message.contains("`to`"), "{message}");
    // The gateway writes these; nobody posts them.
    for kind in ["notice", "person_answer", "person_message", "done"] {
        let message = refusal::<TeamPost>(json!({"type": kind, "text": "x"}))?;
        assert!(message.contains(kind), "{message}");
    }
    Ok(())
}

#[test]
fn proto_u_023_start_member_defaults_to_its_own_copy_and_takes_an_optional_profile() -> TestResult {
    let brief = json!({"objective": "Find the Q3 revenue.", "output": "A table.", "boundaries": "Write only results.md."});
    let wire = json!({"role": "researcher", "brief": brief, "workspace": "own_copy"});
    round_trip::<StartMember>(&wire)?;
    let start: StartMember = serde_json::from_value(json!({"role": "researcher", "brief": brief}))?;
    assert_eq!(start.workspace, WorkspaceMode::OwnCopy);
    assert_eq!(start.profile, None);
    assert_eq!(start.brief.sources, None);
    round_trip::<StartMember>(&json!({
        "role": "legal reviewer",
        "brief": {"objective": "Check the contract.", "output": "A list of risks.", "sources": "contract.pdf only.",
                  "boundaries": "Don't edit the contract."},
        "workspace": "my_files",
        "profile": "legal-reviewer"}))?;
    let message =
        refusal::<StartMember>(json!({"role": "r", "brief": brief, "prompt": "old name"}))?;
    assert!(message.contains("`prompt`"), "{message}");
    let message =
        refusal::<StartMember>(json!({"role": "r", "brief": brief, "workspace": "shared"}))?;
    assert!(message.contains("shared"), "{message}");
    Ok(())
}

#[test]
fn proto_u_023_a_brief_needs_its_objective_output_and_boundaries() -> TestResult {
    // Proposal A (ADR-0034, decided 2 Oct 2026): the three parts are required; the sources are optional.
    for (brief, field) in [
        (json!({"output": "o", "boundaries": "b"}), "objective"),
        (json!({"objective": "o", "boundaries": "b"}), "output"),
        (json!({"objective": "o", "output": "o"}), "boundaries"),
    ] {
        let message = refusal::<StartMember>(json!({"role": "r", "brief": brief}))?;
        assert!(message.contains(field), "{message}");
    }
    Ok(())
}

#[test]
fn proto_u_023_take_files_names_a_report_and_optionally_its_files() -> TestResult {
    round_trip::<TakeFiles>(&json!({"report_seq": 9}))?;
    round_trip::<TakeFiles>(&json!({"report_seq": 9, "files": ["src/report.md"]}))?;
    let all: TakeFiles = serde_json::from_value(json!({"report_seq": 9, "files": null}))?;
    assert_eq!(all.files, None, "null means all of the report's files");
    round_trip::<FilesTaken>(&json!({"placed": [
        {"name": "src/report.md", "path": "uploads/researcher-src-report.md", "size": 2104}
    ]}))?;
    // Placement is the gateway's: the slice 1 draft's `into` and `replace` are gone (2 Oct 2026).
    assert!(refusal::<TakeFiles>(json!({"report_seq": 9, "into": "helpers/"}))?.contains("into"));
    assert!(refusal::<TakeFiles>(json!({"report_seq": 9, "replace": true}))?.contains("replace"));
    assert!(refusal::<TakeFiles>(json!({"files": []}))?.contains("report_seq"));
    Ok(())
}

#[test]
fn proto_u_023_notice_kinds_have_their_wire_names() -> TestResult {
    for (kind, name) in [
        (NoticeKind::MemberIdle, "member_idle"),
        (NoticeKind::MemberStuck, "member_stuck"),
        (NoticeKind::MemberFailed, "member_failed"),
        (NoticeKind::MemberStopped, "member_stopped"),
        (NoticeKind::MemberDeleted, "member_deleted"),
        (NoticeKind::MemberQueued, "member_queued"),
        (NoticeKind::Budget80, "budget_80"),
        (NoticeKind::DeadlineNear, "deadline_near"),
    ] {
        assert_eq!(serde_json::to_value(kind)?, json!(name));
    }
    Ok(())
}

#[test]
fn proto_u_023_the_lead_brief_rides_a_members_first_turn_and_its_journal_entry() -> TestResult {
    let brief = json!({
        "role": "researcher", "workspace": "own_copy",
        "brief": {"objective": "Find the Q3 revenue.", "output": "A table.", "boundaries": "Read only."},
    });
    // The turn the gateway delivers, and the journal entry the runtime writes for it, carry the same brief.
    let frame = json!({"type": "user.message", "text": "You are researcher, a helper of this task's lead.",
                       "attachments": [], "lead_brief": brief});
    let parsed: tqa_types::ClientFrame = serde_json::from_value(frame.clone())?;
    assert_eq!(serde_json::to_value(&parsed)?, frame);
    let event = json!({"type": "message.user", "text": "You are researcher, a helper of this task's lead.",
                       "attachments": [], "lead_brief": brief});
    let parsed: tqa_types::Event = serde_json::from_value(event.clone())?;
    assert_eq!(serde_json::to_value(&parsed)?, event);
    // A person's message has none, and none is written.
    let plain: tqa_types::Event =
        serde_json::from_value(json!({"type": "message.user", "text": "hi", "attachments": []}))?;
    assert!(serde_json::to_value(&plain)?.get("lead_brief").is_none());
    // The brief is strict: an unknown field in it is refused.
    let mut odd = frame;
    odd["lead_brief"]["tone"] = json!("x");
    assert!(serde_json::from_value::<tqa_types::ClientFrame>(odd).is_err());
    Ok(())
}

// TEST: PROTO-U-035
#[test]
fn proto_u_035_a_wake_marks_its_turn_and_journal_entry_with_the_readers_place() -> TestResult {
    // The gateway's wake (revision 13, ADR-0034 Waking): the turn and the journal entry carry the reader's place, so a
    // client shows the gateway's notice in its own words, never the person's message. It names no writer: the log does.
    let text = "New messages from your lead since message 7. Read them with wait_for_lead.";
    let frame =
        json!({"type": "user.message", "text": text, "attachments": [], "team_wake": {"after": 7}});
    let parsed: tqa_types::ClientFrame = serde_json::from_value(frame.clone())?;
    assert_eq!(serde_json::to_value(&parsed)?, frame);
    let event =
        json!({"type": "message.user", "text": text, "attachments": [], "team_wake": {"after": 0}});
    let parsed: tqa_types::Event = serde_json::from_value(event.clone())?;
    assert_eq!(serde_json::to_value(&parsed)?, event);
    // A person's message has none, and none is written.
    let plain: tqa_types::ClientFrame =
        serde_json::from_value(json!({"type": "user.message", "text": "hi", "attachments": []}))?;
    assert!(serde_json::to_value(&plain)?.get("team_wake").is_none());
    // Strict: a negative place, no place, or any other field (a writer included) is refused.
    for odd in [
        json!({"after": -1}),
        json!({}),
        json!({"after": 1, "from": "lead"}),
    ] {
        let mut frame = frame.clone();
        frame["team_wake"] = odd.clone();
        assert!(
            serde_json::from_value::<tqa_types::ClientFrame>(frame).is_err(),
            "{odd}"
        );
    }
    // The schema has the place alone.
    let schema = tqa_types::protocol_schema();
    let wake = &schema["definitions"]["TeamWake"];
    assert_eq!(wake["required"], json!(["after"]), "{wake}");
    assert!(schema["definitions"].get("WakeSender").is_none());
    Ok(())
}

#[test]
fn proto_u_023_dones_check_report_round_trips_and_its_output_keeps_its_end() -> TestResult {
    round_trip::<CheckReport>(
        &json!({"command": "cargo test", "exit_code": 1, "timed_out": false,
        "output": "[stderr] test failed"}),
    )?;
    round_trip::<CheckReport>(
        &json!({"command": "make check", "exit_code": null, "timed_out": true, "output": ""}),
    )?;
    let error = refusal::<CheckReport>(
        json!({"command": "true", "exit_code": 0, "timed_out": false,
        "output": "", "passed": true}),
    )?;
    assert!(error.contains("passed"), "{error}");
    // The last bytes, never splitting a character: the four-byte one that would be cut is left out whole.
    assert_eq!(tqa_types::last_bytes("abcdef", 3), "def");
    assert_eq!(tqa_types::last_bytes("ab", 3), "ab");
    assert_eq!(tqa_types::last_bytes("a\u{1F600}b", 4), "b");
    assert_eq!(tqa_types::last_bytes("a\u{1F600}b", 5), "\u{1F600}b");
    assert_eq!(tqa_types::MAX_CHECK_OUTPUT_BYTES, 4_096);
    Ok(())
}
