//! GW-U-125: `start_member`'s brief and the agent-team posts are validated naming the field and the reason (ADR-0034,
//! the slice S1 contract).
use super::{unique_name, validate_done, validate_post, validate_start};
use crate::error::{GatewayError, InvalidReason};
use tqa_types::{
    Brief, MAX_BRIEF_PART_BYTES, MAX_ROLE_BYTES, StartMember, TeamMessageType, TeamPost,
    WorkspaceMode,
};

fn start(role: &str, objective: &str) -> StartMember {
    StartMember {
        role: role.into(),
        brief: Brief {
            objective: objective.into(),
            output: "A table.".into(),
            sources: None,
            boundaries: "Write only results.md.".into(),
        },
        workspace: WorkspaceMode::OwnCopy,
        profile: None,
    }
}

fn invalid(result: Result<(), GatewayError>) -> Option<(&'static str, InvalidReason)> {
    match result.err() {
        Some(GatewayError::Invalid { field, reason }) => Some((field, reason)),
        _ => None,
    }
}

fn unknown(result: Result<(), GatewayError>) -> Option<&'static str> {
    match result.err() {
        Some(GatewayError::UnknownReference { field }) => Some(field),
        _ => None,
    }
}

#[test]
fn gw_u_125_a_brief_at_its_limits_is_accepted() {
    let mut request = start(
        &"r".repeat(MAX_ROLE_BYTES),
        &"o".repeat(MAX_BRIEF_PART_BYTES),
    );
    request.brief.output = "p".repeat(MAX_BRIEF_PART_BYTES);
    request.brief.sources = Some("s".repeat(MAX_BRIEF_PART_BYTES));
    request.profile = Some("legal-reviewer".into());
    assert!(validate_start(&request).is_ok());
    assert!(validate_start(&start("legal reviewer_2-b", "Check the contract.")).is_ok());
    assert!(validate_start(&start("Prüfer", "Unicode letters are letters.")).is_ok());
}

#[test]
fn gw_u_125_each_brief_rule_names_its_field_and_reason() {
    use InvalidReason::{NameInvalid, ParameterInvalid, ValueEmpty, ValueNul, ValueTooLong};
    let over = "x".repeat(MAX_BRIEF_PART_BYTES + 1);
    let cases: Vec<(StartMember, &str, InvalidReason)> = vec![
        (start("", "o"), "role", ValueEmpty),
        (
            start(&"r".repeat(MAX_ROLE_BYTES + 1), "o"),
            "role",
            ValueTooLong,
        ),
        (start("two\nlines", "o"), "role", NameInvalid),
        (start("slash/role", "o"), "role", NameInvalid),
        (start("nul\0role", "o"), "role", ValueNul),
        (start("r", ""), "brief.objective", ValueEmpty),
        (start("r", &over), "brief.objective", ValueTooLong),
        (start("r", "nul\0"), "brief.objective", ValueNul),
        (
            StartMember {
                brief: Brief {
                    output: over.clone(),
                    ..start("r", "o").brief
                },
                ..start("r", "o")
            },
            "brief.output",
            ValueTooLong,
        ),
        (
            StartMember {
                brief: Brief {
                    boundaries: "nul\0".into(),
                    ..start("r", "o").brief
                },
                ..start("r", "o")
            },
            "brief.boundaries",
            ValueNul,
        ),
        (
            StartMember {
                brief: Brief {
                    output: String::new(),
                    ..start("r", "o").brief
                },
                ..start("r", "o")
            },
            "brief.output",
            ValueEmpty,
        ),
        (
            StartMember {
                brief: Brief {
                    boundaries: String::new(),
                    ..start("r", "o").brief
                },
                ..start("r", "o")
            },
            "brief.boundaries",
            ValueEmpty,
        ),
        (
            StartMember {
                profile: Some("Legal Reviewer".into()),
                ..start("r", "o")
            },
            "profile",
            ParameterInvalid,
        ),
    ];
    for (request, field, reason) in cases {
        assert_eq!(
            invalid(validate_start(&request)),
            Some((field, reason)),
            "{request:?}"
        );
    }
}

#[test]
fn gw_u_125_a_brief_over_its_whole_limit_is_refused_as_the_brief() {
    let part = "x".repeat(MAX_BRIEF_PART_BYTES);
    let mut request = start("r", &part);
    request.brief.output = part.clone();
    request.brief.sources = Some(part.clone());
    request.brief.boundaries = part;
    assert_eq!(
        invalid(validate_start(&request)),
        Some(("brief", InvalidReason::ValueTooLong))
    );
}

/// Builds a post of one type around its text.
type MakePost = fn(String) -> TeamPost;

fn refs(seq: u64) -> Option<TeamMessageType> {
    match seq {
        5 => Some(TeamMessageType::Question),
        6 => Some(TeamMessageType::Report),
        7 => Some(TeamMessageType::Ask),
        8 => Some(TeamMessageType::PersonQuestion),
        _ => None,
    }
}

#[test]
fn gw_u_125_each_post_type_is_held_to_its_text_limit() {
    for post in [
        TeamPost::Question {
            text: String::new(),
        },
        TeamPost::Ask {
            text: String::new(),
        },
        TeamPost::Assign {
            to: "researcher".into(),
            text: String::new(),
        },
    ] {
        assert_eq!(
            invalid(validate_post(&post, refs)),
            Some(("text", InvalidReason::ValueEmpty)),
            "{post:?}"
        );
    }
    let at_limit = |kind: TeamMessageType| "x".repeat(kind.max_text_bytes());
    let over_limit = |kind: TeamMessageType| "x".repeat(kind.max_text_bytes() + 1);
    let cases: [(TeamMessageType, MakePost); 4] = [
        (TeamMessageType::Question, |text: String| {
            TeamPost::Question { text }
        }),
        (TeamMessageType::Ask, |text: String| TeamPost::Ask { text }),
        (TeamMessageType::PersonQuestion, |text: String| {
            TeamPost::PersonQuestion { text }
        }),
        (TeamMessageType::Report, |text: String| TeamPost::Report {
            text,
            files: vec![],
        }),
    ];
    for (kind, make) in cases {
        assert!(
            validate_post(&make(at_limit(kind)), refs).is_ok(),
            "{kind:?}"
        );
        assert_eq!(
            invalid(validate_post(&make(over_limit(kind)), refs)),
            Some(("text", InvalidReason::ValueTooLong)),
            "{kind:?}"
        );
    }
    assert_eq!(
        invalid(validate_post(
            &TeamPost::Ask {
                text: "nul\0".into()
            },
            refs
        )),
        Some(("text", InvalidReason::ValueNul))
    );
    assert_eq!(
        invalid(validate_post(
            &TeamPost::Assign {
                to: String::new(),
                text: "hi".into()
            },
            refs
        )),
        Some(("to", InvalidReason::ValueEmpty))
    );
}

#[test]
fn gw_u_125_a_reference_must_name_a_message_of_the_right_type() {
    let answer = |question_seq| TeamPost::Answer {
        question_seq,
        text: "CSV.".into(),
    };
    assert!(validate_post(&answer(5), refs).is_ok());
    assert_eq!(
        unknown(validate_post(&answer(6), refs)),
        Some("question_seq")
    );
    assert_eq!(
        unknown(validate_post(&answer(99), refs)),
        Some("question_seq")
    );
    // A member doesn't answer another member's ask, nor the person's question.
    for seq in [7, 8] {
        assert_eq!(
            unknown(validate_post(&answer(seq), refs)),
            Some("question_seq"),
            "{seq}"
        );
    }
    let decision = |question_seq| TeamPost::Decision {
        text: "Use CSV.".into(),
        question_seq,
    };
    assert!(validate_post(&decision(None), refs).is_ok());
    assert!(validate_post(&decision(Some(5)), refs).is_ok());
    // The lead decides a member's ask too; a report, the person's question or no message at all is refused.
    assert!(validate_post(&decision(Some(7)), refs).is_ok());
    for seq in [6, 8, 99] {
        assert_eq!(
            unknown(validate_post(&decision(Some(seq)), refs)),
            Some("question_seq"),
            "{seq}"
        );
    }
}

#[test]
fn gw_u_125_a_report_names_at_most_its_files_with_safe_paths() {
    let report = |files: Vec<&str>| TeamPost::Report {
        text: "Done.".into(),
        files: files.into_iter().map(String::from).collect(),
    };
    assert!(validate_post(&report(vec!["results.md", "charts/q3.png"]), refs).is_ok());
    for path in ["", "/etc/passwd", "../outside.md", "a/../../b", "nul\0.md"] {
        assert_eq!(
            invalid(validate_post(&report(vec![path]), refs)).map(|(field, _)| field),
            Some("files"),
            "{path:?}"
        );
    }
    let many: Vec<String> = (0..=super::MAX_REPORT_FILES)
        .map(|n| format!("f{n}.md"))
        .collect();
    assert_eq!(
        invalid(validate_post(
            &report(many.iter().map(String::as_str).collect()),
            refs
        )),
        Some(("files", InvalidReason::ValueTooLong))
    );
}

#[test]
fn gw_u_125_a_taken_role_gets_the_next_number() {
    assert_eq!(unique_name("researcher", &[]), "researcher");
    assert_eq!(unique_name("researcher", &["reviewer"]), "researcher");
    assert_eq!(unique_name("researcher", &["researcher"]), "researcher-2");
    assert_eq!(
        unique_name(
            "researcher",
            &["researcher", "researcher-2", "researcher-3"]
        ),
        "researcher-4"
    );
    // The first free number, past a gap.
    assert_eq!(
        unique_name("researcher", &["researcher", "researcher-3"]),
        "researcher-2"
    );
    assert_eq!(unique_name("Researcher", &["researcher"]), "Researcher-2");
    assert_eq!(
        unique_name("Prüfer", &["PRÜFER"]),
        "Prüfer-2",
        "non-ASCII letters fold as lower() does"
    );
}

#[test]
fn gw_u_125_dones_summary_is_held_to_the_done_messages_limit() {
    let limit = TeamMessageType::Done.max_text_bytes();
    assert!(validate_done(&"s".repeat(limit)).is_ok());
    for (summary, reason) in [
        (String::new(), InvalidReason::ValueEmpty),
        ("s".repeat(limit + 1), InvalidReason::ValueTooLong),
        ("a\0b".to_owned(), InvalidReason::ValueNul),
    ] {
        assert_eq!(
            invalid(validate_done(&summary)),
            Some(("summary", reason)),
            "{} bytes",
            summary.len()
        );
    }
}
