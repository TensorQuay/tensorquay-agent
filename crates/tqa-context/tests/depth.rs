//! CTX-U-009: the last N turns of a stored log, for a client that shows recent history first.
#[allow(
    dead_code,
    reason = "This suite uses only the frame builder from the shared vocabulary."
)]
#[path = "support/history.rs"]
mod support;
use support::frames;
use tqa_context::recent_turns;
use tqa_types::{Event, ServerFrame};

fn user(text: &str) -> Event {
    Event::MessageUser {
        lead_brief: None,
        team_wake: None,
        text: text.into(),
        attachments: vec![],
    }
}
fn texts(frames: &[ServerFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|frame| match frame {
            ServerFrame::Event {
                event: Event::MessageUser { text, .. },
                ..
            } => Some(format!("user:{text}")),
            ServerFrame::Event {
                event: Event::TextDelta { text },
                ..
            } => Some(format!("text:{text}")),
            _ => None,
        })
        .collect()
}

#[test]
fn ctx_u_009_turns_start_at_each_user_message_and_orphans_are_dropped() {
    let log = frames(vec![
        // Assistant output before any user message belongs to no turn.
        Event::TextDelta {
            text: "orphan".into(),
        },
        user("one"),
        Event::TextDelta {
            text: "reply one".into(),
        },
        user("two"),
        // Consecutive user messages are separate turns.
        user("three"),
        Event::TextDelta {
            text: "reply three".into(),
        },
    ]);
    assert_eq!(
        texts(recent_turns(&log, 1)),
        ["user:three", "text:reply three"]
    );
    assert_eq!(
        texts(recent_turns(&log, 2)),
        ["user:two", "user:three", "text:reply three"]
    );
    let all = [
        "user:one",
        "text:reply one",
        "user:two",
        "user:three",
        "text:reply three",
    ];
    assert_eq!(texts(recent_turns(&log, 3)), all);
    assert_eq!(
        texts(recent_turns(&log, 10)),
        all,
        "fewer turns than requested gives every turn"
    );
    assert!(recent_turns(&log, 0).is_empty(), "zero turns gives nothing");
    assert!(
        recent_turns(
            &frames(vec![Event::TextDelta {
                text: "only orphans".into()
            }]),
            5
        )
        .is_empty()
    );
}
