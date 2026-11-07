//! GW-U-078: the messages held behind a run become one user turn: in order, within one turn's bounds.
use super::{Part, join};
use tqa_types::{ClientFrame, MAX_ATTACHMENTS, MAX_USER_MESSAGE_BYTES};

fn part(text: &str, attachments: &[&str]) -> Part {
    Part {
        text: text.to_owned(),
        attachments: attachments.iter().map(|a| (*a).to_owned()).collect(),
        lead_brief: None,
        team_wake: None,
    }
}

// TEST: GW-U-078
#[test]
fn gw_u_078_held_messages_join_in_order_with_a_blank_line_and_each_attachment_once() {
    let (taken, turn) = join(&[
        part("also check the tests", &["uploads/a.png"]),
        part(
            "and update the changelog",
            &["uploads/b.log", "uploads/a.png"],
        ),
    ]);
    assert_eq!(taken, 2);
    assert_eq!(
        turn,
        ClientFrame::UserMessage {
            lead_brief: None,
            team_wake: None,
            text: "also check the tests\n\nand update the changelog".into(),
            attachments: vec!["uploads/a.png".into(), "uploads/b.log".into()],
            host_keys: Vec::new(),
        }
    );
    // One message is sent as it was written.
    let (taken, turn) = join(&[part("only this", &[])]);
    assert_eq!(taken, 1);
    assert_eq!(
        turn,
        ClientFrame::UserMessage {
            lead_brief: None,
            team_wake: None,
            text: "only this".into(),
            attachments: vec![],
            host_keys: Vec::new(),
        }
    );
}

// TEST: GW-U-078
#[test]
fn gw_u_078_a_turn_stops_where_the_next_message_would_not_fit()
-> Result<(), Box<dyn std::error::Error>> {
    // Text: two messages that each fit but not together (with the separator) make two turns.
    let half = "x".repeat(MAX_USER_MESSAGE_BYTES / 2);
    let (taken, _) = join(&[part(&half, &[]), part(&half, &[]), part("third", &[])]);
    assert_eq!(taken, 1, "the second would pass the bound by the separator");
    // Attachments: at most the protocol's number, counted once each.
    let many: Vec<String> = (0..MAX_ATTACHMENTS)
        .map(|i| format!("uploads/{i}"))
        .collect();
    let refs: Vec<&str> = many.iter().map(String::as_str).collect();
    let (taken, turn) = join(&[
        part("all of them", &refs),
        part("the same again", &refs),
        part("one more", &["uploads/extra"]),
    ]);
    assert_eq!(
        taken, 2,
        "repeated attachments count once; a new one doesn't fit"
    );
    let ClientFrame::UserMessage { attachments, .. } = turn else {
        return Err("a user turn".into());
    };
    assert_eq!(attachments.len(), MAX_ATTACHMENTS);
    // Each turn is a valid frame on the wire.
    let (_, turn) = join(&[part(&half, &[]), part(&half[1..], &[])]);
    assert!(ClientFrame::parse(&serde_json::to_vec(&turn)?).is_ok());
    Ok(())
}

// TEST: GW-U-145
#[test]
fn gw_u_145_a_wake_is_a_turn_of_its_own_with_its_marker() {
    let wake = tqa_types::TeamWake { after: 4 };
    let woken = Part {
        team_wake: Some(wake),
        ..part("New messages from your lead since message 4.", &[])
    };
    // The wake goes alone, marked, even with a person's message waiting behind it: joined, the person's words would
    // show as the gateway's notice, or the notice as the person's words.
    let (taken, turn) = join(&[woken.clone(), part("a person's message", &[])]);
    assert_eq!(taken, 1);
    assert_eq!(
        turn,
        ClientFrame::UserMessage {
            lead_brief: None,
            team_wake: Some(wake),
            text: "New messages from your lead since message 4.".into(),
            attachments: Vec::new(),
            host_keys: Vec::new(),
        }
    );
    // A person's message ahead of it goes without it; the wake waits for its own turn.
    let (taken, turn) = join(&[part("earlier", &[]), woken]);
    assert_eq!(taken, 1);
    assert!(matches!(
        turn,
        ClientFrame::UserMessage {
            team_wake: None,
            ..
        }
    ));
}

// TEST: GW-U-078
#[test]
fn gw_u_078_a_members_first_turn_is_a_turn_of_its_own_with_its_brief() {
    let brief = tqa_types::LeadBrief {
        role: "researcher".into(),
        workspace: tqa_types::WorkspaceMode::MyFiles,
        brief: tqa_types::Brief {
            objective: "o".into(),
            output: "p".into(),
            sources: None,
            boundaries: "b".into(),
        },
    };
    let first = Part {
        lead_brief: Some(brief.clone()),
        ..part("the frame", &[])
    };
    // The brief's turn goes alone, with its brief, even with a person's message waiting behind it.
    let (taken, turn) = join(&[first.clone(), part("a person's message", &[])]);
    assert_eq!(taken, 1);
    assert_eq!(
        turn,
        ClientFrame::UserMessage {
            lead_brief: Some(brief),
            team_wake: None,
            text: "the frame".into(),
            attachments: Vec::new(),
            host_keys: Vec::new(),
        }
    );
    // A person's message ahead of it goes without it; the brief waits for its own turn.
    let (taken, turn) = join(&[part("earlier", &[]), first]);
    assert_eq!(taken, 1);
    assert!(matches!(
        turn,
        ClientFrame::UserMessage {
            lead_brief: None,
            team_wake: None,
            ..
        }
    ));
}
