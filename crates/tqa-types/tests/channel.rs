//! PROTO-U-020: channel messages (ADR-0014): each message type's JSON shape, what a member may post (never a
//! `decision`, which the gateway writes, or a `claim`, which only `task_claim` makes), the bounds, the roles and the
//! read and wait filter.
use serde_json::{Value, json};
use tqa_types::{
    AuthorKind, ChannelFilter, ChannelMessage, ChannelPost, ChannelRole, ClaimLimit,
    MAX_CHANNEL_BODY_BYTES, MessageBody, MessageType,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn post(value: &Value) -> Result<ChannelPost, tqa_types::FrameError> {
    ChannelPost::parse(value.to_string().as_bytes())
}

fn refused_at(value: &Value) -> String {
    post(value).map_or_else(|error| error.path, |_| "accepted".into())
}

// TEST: PROTO-U-020
#[test]
fn proto_u_020_every_message_type_has_its_json_shape() -> TestResult {
    let bodies = [
        json!({"type": "text", "text": "hello"}),
        json!({"type": "proposal", "title": "Use SQLite", "text": "Because…", "window_seconds": 600}),
        json!({"type": "vote", "proposal_seq": 3, "choice": "accept", "reason": "simpler"}),
        json!({"type": "vote", "proposal_seq": 3, "choice": "veto"}),
        json!({"type": "decision", "proposal_seq": 3, "outcome": "timed_out", "rule": "majority"}),
        json!({"type": "task", "title": "Write the parser", "spec": "In crates/x"}),
        json!({"type": "task", "title": "Compare", "spec": "Each try it", "claims": 3}),
        json!({"type": "task", "title": "Everyone", "spec": "All of you", "claims": "all"}),
        json!({"type": "claim", "task_seq": 5}),
        json!({"type": "report", "task_seq": 5, "status": "completed", "summary": "Done", "refs": ["commit abc123"]}),
        json!({"type": "done", "summary": "Finished"}),
        json!({"type": "done"}),
    ];
    for body in bodies {
        let parsed: MessageBody = serde_json::from_value(body.clone())?;
        assert_eq!(serde_json::to_value(&parsed)?, body, "round trip");
        assert_eq!(
            serde_json::to_value(parsed.message_type())?,
            body["type"],
            "{body}"
        );
    }
    let message: ChannelMessage = serde_json::from_value(json!({
        "seq": 7, "ts": "2026-09-27T10:00:00Z", "parent": 3,
        "author": {"kind": "conversation", "id": "conversation_1", "name": "planner"},
        "body": {"type": "text", "text": "hi"}
    }))?;
    assert_eq!(
        (message.seq, message.parent, message.author.kind),
        (7, Some(3), AuthorKind::Conversation)
    );
    Ok(())
}

// TEST: PROTO-U-020
#[test]
fn proto_u_020_a_member_posts_anything_but_a_decision_or_a_claim() -> TestResult {
    for body in [
        json!({"type": "text", "text": "hello"}),
        json!({"type": "proposal", "title": "t", "text": "x"}),
        json!({"type": "vote", "proposal_seq": 1, "choice": "reject"}),
        json!({"type": "task", "title": "t", "spec": "s"}),
        json!({"type": "report", "task_seq": 1, "status": "blocked", "summary": "s"}),
        json!({"type": "done"}),
    ] {
        post(&json!({"body": body, "parent": 1}))?;
    }
    assert_eq!(
        refused_at(
            &json!({"body": {"type": "decision", "proposal_seq": 1, "outcome": "accepted", "rule": "human"}})
        ),
        "body.type"
    );
    assert_eq!(
        refused_at(&json!({"body": {"type": "claim", "task_seq": 1}})),
        "body.type"
    );
    // A forged author is accepted and dropped: the gateway sets the author from the token (SEC-015).
    let forged = post(
        &json!({"body": {"type": "text", "text": "hi"}, "author": {"kind": "user", "id": "admin"}}),
    )?;
    assert!(!serde_json::to_string(&forged)?.contains("admin"));
    // Anything else unknown is refused, as is an unknown type.
    assert_ne!(
        refused_at(&json!({"body": {"type": "text", "text": "hi"}, "extra": 1})),
        "accepted"
    );
    assert_ne!(
        refused_at(&json!({"body": {"type": "text", "text": "hi", "extra": 1}})),
        "accepted"
    );
    assert_ne!(
        refused_at(&json!({"body": {"type": "custom", "schema_id": "x"}})),
        "accepted"
    );
    Ok(())
}

// TEST: PROTO-U-020
#[test]
fn proto_u_020_bounds_are_checked_naming_the_field() {
    let long = "x".repeat(MAX_CHANNEL_BODY_BYTES);
    let cases = [
        (json!({"body": {"type": "text", "text": ""}}), "body.text"),
        (json!({"body": {"type": "text", "text": long}}), "body"),
        (
            json!({"body": {"type": "proposal", "title": "", "text": "x"}}),
            "body.title",
        ),
        (
            json!({"body": {"type": "proposal", "title": "t".repeat(201), "text": "x"}}),
            "body.title",
        ),
        (
            json!({"body": {"type": "proposal", "title": "a\u{7}b", "text": "x"}}),
            "body.title",
        ),
        (
            json!({"body": {"type": "proposal", "title": "t", "text": "x", "window_seconds": 59}}),
            "body.window_seconds",
        ),
        (
            json!({"body": {"type": "proposal", "title": "t", "text": "x", "window_seconds": 86_401}}),
            "body.window_seconds",
        ),
        (
            json!({"body": {"type": "vote", "proposal_seq": 0, "choice": "accept"}}),
            "body.proposal_seq",
        ),
        (
            json!({"body": {"type": "vote", "proposal_seq": 1, "choice": "accept", "reason": "r".repeat(1001)}}),
            "body.reason",
        ),
        (
            json!({"body": {"type": "task", "title": "t", "spec": ""}}),
            "body.spec",
        ),
        (
            json!({"body": {"type": "task", "title": "t", "spec": "s", "claims": 0}}),
            "body.claims",
        ),
        (
            json!({"body": {"type": "task", "title": "t", "spec": "s", "claims": 33}}),
            "body.claims",
        ),
        (
            json!({"body": {"type": "report", "task_seq": 1, "status": "completed", "summary": "s", "refs": vec!["r"; 17]}}),
            "body.refs",
        ),
        (
            json!({"body": {"type": "report", "task_seq": 1, "status": "completed", "summary": "s", "refs": ["r".repeat(513)]}}),
            "body.refs",
        ),
        (
            json!({"body": {"type": "text", "text": "hi"}, "parent": 0}),
            "parent",
        ),
    ];
    for (value, field) in cases {
        assert_eq!(refused_at(&value), field, "{value}");
    }
    // At the bounds: accepted.
    for value in [
        json!({"body": {"type": "proposal", "title": "t".repeat(200), "text": "x", "window_seconds": 60}}),
        json!({"body": {"type": "proposal", "title": "t", "text": "x", "window_seconds": 86_400}}),
        json!({"body": {"type": "report", "task_seq": 1, "status": "failed", "summary": "s", "refs": vec!["r".repeat(512); 16]}}),
        json!({"body": {"type": "task", "title": "t", "spec": "s", "claims": 32}}),
    ] {
        assert_eq!(refused_at(&value), "accepted", "{value}");
    }
}

// TEST: PROTO-U-020
#[test]
fn proto_u_020_leads_and_members_post_and_vote_observers_only_read() {
    for (role, writes) in [
        (ChannelRole::Lead, true),
        (ChannelRole::Member, true),
        (ChannelRole::Observer, false),
    ] {
        assert_eq!(
            (role.may_post(), role.may_vote()),
            (writes, writes),
            "{role:?}"
        );
    }
}

// TEST: PROTO-U-020
#[test]
fn proto_u_020_the_filter_selects_by_type_thread_task_and_author() -> TestResult {
    let message = |seq: u64,
                   parent: Option<u64>,
                   kind: &str,
                   body: Value|
     -> Result<ChannelMessage, serde_json::Error> {
        serde_json::from_value(
            json!({"seq": seq, "ts": "2026-09-27T10:00:00Z", "parent": parent,
            "author": {"kind": kind, "id": "a"}, "body": body}),
        )
    };
    let task = message(
        5,
        None,
        "user",
        json!({"type": "task", "title": "t", "spec": "s"}),
    )?;
    let claim = message(
        6,
        None,
        "conversation",
        json!({"type": "claim", "task_seq": 5}),
    )?;
    let reply = message(
        7,
        Some(5),
        "conversation",
        json!({"type": "text", "text": "on it"}),
    )?;
    let other = message(
        8,
        None,
        "gateway",
        json!({"type": "decision", "proposal_seq": 2, "outcome": "accepted", "rule": "human"}),
    )?;
    let all = [&task, &claim, &reply, &other];
    let selected = |filter: Value| -> Result<Vec<u64>, Box<dyn std::error::Error>> {
        let filter = ChannelFilter::parse(filter.to_string().as_bytes())?;
        Ok(all
            .iter()
            .filter(|message| filter.matches(message))
            .map(|message| message.seq)
            .collect())
    };
    assert_eq!(selected(json!({}))?, [5, 6, 7, 8]);
    assert_eq!(selected(json!({"types": ["claim", "decision"]}))?, [6, 8]);
    assert_eq!(selected(json!({"parent": 5}))?, [7]);
    // A task's own message, its claim and its reports.
    assert_eq!(selected(json!({"task_seq": 5}))?, [5, 6]);
    assert_eq!(selected(json!({"author_kind": "conversation"}))?, [6, 7]);
    assert_eq!(
        selected(json!({"types": ["text"], "author_kind": "conversation"}))?,
        [7]
    );
    assert!(
        ChannelFilter::parse(br#"{"since": 3}"#).is_err(),
        "unknown field"
    );
    assert!(
        ChannelFilter::parse(br#"{"types": ["custom"]}"#).is_err(),
        "unknown type"
    );
    assert!(ChannelFilter::parse(br#"{"parent": 0}"#).is_err(), "seq 0");
    assert_eq!(MessageType::ALL.len(), 8);
    Ok(())
}

// TEST: PROTO-U-020
#[test]
fn proto_u_020_a_task_takes_one_claimant_by_default_a_number_or_all() -> TestResult {
    let limit = |body: Value| -> Result<ClaimLimit, Box<dyn std::error::Error>> {
        match serde_json::from_value(body)? {
            MessageBody::Task { claims, .. } => Ok(claims),
            other => Err(format!("not a task: {other:?}").into()),
        }
    };
    assert_eq!(
        limit(json!({"type": "task", "title": "t", "spec": "s"}))?,
        ClaimLimit::Count(1)
    );
    assert_eq!(
        limit(json!({"type": "task", "title": "t", "spec": "s", "claims": 4}))?,
        ClaimLimit::Count(4)
    );
    assert_eq!(
        limit(json!({"type": "task", "title": "t", "spec": "s", "claims": "all"}))?,
        ClaimLimit::All
    );
    // How many claims a task takes, given its channel's voting members.
    assert_eq!(
        (
            ClaimLimit::Count(1).of(5),
            ClaimLimit::Count(4).of(3),
            ClaimLimit::All.of(5)
        ),
        (1, 4, 5)
    );
    for claims in [json!("some"), json!(-1), json!(1.5), json!(null)] {
        assert!(
            serde_json::from_value::<MessageBody>(
                json!({"type": "task", "title": "t", "spec": "s", "claims": claims})
            )
            .is_err(),
            "{claims}"
        );
    }
    Ok(())
}

// TEST: PROTO-U-048
// The gateway's wake of a channel member (ADR-0014's amendment of 20 Nov 2026, A4; revision 16): user.message and its
// journaled message.user carry the channel and the member's place, nothing else, so a client shows the gateway's
// notice and never a person's message; a person's message has none.
#[test]
fn proto_u_048_a_channel_wake_is_the_channel_and_the_place_alone() -> TestResult {
    let channel = "0194f0a2-7c3e-7d1a-9b2c-3d4e5f607182";
    let frame = json!({"type": "user.message", "text": "New in channel", "attachments": [],
        "channel_wake": {"channel": channel, "after": 11}});
    let parsed: tqa_types::ClientFrame = serde_json::from_value(frame.clone())?;
    assert_eq!(serde_json::to_value(&parsed)?, frame);
    let event = json!({"type": "message.user", "text": "New in channel", "attachments": [],
        "channel_wake": {"channel": channel, "after": 0}});
    let parsed: tqa_types::Event = serde_json::from_value(event.clone())?;
    assert_eq!(serde_json::to_value(&parsed)?, event);
    let plain: tqa_types::ClientFrame =
        serde_json::from_value(json!({"type": "user.message", "text": "hi", "attachments": []}))?;
    assert!(serde_json::to_value(&plain)?.get("channel_wake").is_none());
    // Strict: no channel, a channel that isn't an id, a negative place, or any other field (a title, a writer).
    for odd in [
        json!({"after": 1}),
        json!({"channel": "a channel", "after": 1}),
        json!({"channel": channel, "after": -1}),
        json!({"channel": channel, "after": 1, "title": "Pricing"}),
    ] {
        let mut frame = frame.clone();
        frame["channel_wake"] = odd.clone();
        assert!(
            serde_json::from_value::<tqa_types::ClientFrame>(frame).is_err(),
            "{odd}"
        );
    }
    let schema = tqa_types::protocol_schema();
    let wake = &schema["definitions"]["ChannelWake"];
    assert_eq!(wake["required"], json!(["channel", "after"]), "{wake}");
    assert_eq!(tqa_types::PROTOCOL_REVISION, 16);
    Ok(())
}
