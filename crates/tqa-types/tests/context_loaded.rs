//! PROTO-U-011: the additive `context.loaded` event round-trips, is a known frame, and validates its bounds.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    ContextLoaded, ErrorCode, Event, LoadedInstruction, LoadedSkill, MAX_CONTEXT_LOADED_ITEMS,
    ServerFrame, SkillSkipReason, SkillTier, SkippedSkill,
};

fn frame(event: &Value) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(
        &json!({"type":"event","seq":3,"run_id":"r-1","ts":"2026-09-26T00:00:00Z","event":event}),
    )
}
fn loaded() -> Value {
    json!({"type":"context.loaded",
        "instructions":[{"path":"AGENTS.md","truncated":false},{"path":"src/AGENTS.md","truncated":true}],
        "skills":[{"name":"pdf-report","tier":"install","description":"Turn a CSV into a PDF report."}],
        "skipped":[{"folder":"Broken Folder","tier":"user","reason":"bad_front_matter"}]})
}

#[test]
fn proto_u_011_context_loaded_round_trips_as_a_known_event() -> Result<(), Box<dyn Error>> {
    let ServerFrame::Event { event, .. } = ServerFrame::parse(&frame(&loaded())?)? else {
        return Err("expected an event frame".into());
    };
    assert!(
        matches!(&event, Event::ContextLoaded { instructions, skills, skipped }
        if instructions.len() == 2 && instructions[1].truncated && skills[0].tier == SkillTier::Install
            && skipped[0].reason == SkillSkipReason::BadFrontMatter)
    );
    assert_eq!(serde_json::to_value(&event)?, loaded());
    let empty = json!({"type":"context.loaded","instructions":[],"skills":[],"skipped":[]});
    assert!(serde_json::from_value::<Event>(empty).is_ok());
    Ok(())
}

#[test]
fn proto_u_011_context_loaded_rejects_out_of_bound_items() -> Result<(), Box<dyn Error>> {
    let item = loaded()["skills"][0].clone();
    let invalid = [
        ("/instructions", json!([{"path":"","truncated":false}])),
        (
            "/instructions",
            json!([{"path":"x".repeat(4097),"truncated":false}]),
        ),
        ("/skills", json!(vec![item; 65])),
        (
            "/skills",
            json!([{"name":"PDF","tier":"install","description":"d"}]),
        ),
        (
            "/skills",
            json!([{"name":"","tier":"install","description":"d"}]),
        ),
        (
            "/skills",
            json!([{"name":"a","tier":"install","description":""}]),
        ),
        (
            "/skills",
            json!([{"name":"a","tier":"install","description":"d".repeat(281)}]),
        ),
        (
            "/skills",
            json!([{"name":"a","tier":"team","description":"d"}]),
        ),
        (
            "/skipped",
            json!([{"folder":"","tier":"user","reason":"symlink"}]),
        ),
        (
            "/skipped",
            json!([{"folder":"f".repeat(256),"tier":"user","reason":"symlink"}]),
        ),
        (
            "/skipped",
            json!([{"folder":"f","tier":"user","reason":"deleted"}]),
        ),
    ];
    for (field, value) in invalid {
        let mut event = loaded();
        event[&field[1..]] = value;
        let error = ServerFrame::parse(&frame(&event)?).err().ok_or(field)?;
        assert_eq!(error.code, ErrorCode::InvalidFrame, "{field}: {error:?}");
    }
    let mut missing = loaded();
    missing.as_object_mut().ok_or("object")?.remove("skipped");
    assert!(
        ServerFrame::parse(&frame(&missing)?).is_err(),
        "every list is required"
    );
    Ok(())
}

#[test]
fn proto_u_011_skill_codes_match_the_wire() -> Result<(), Box<dyn Error>> {
    for reason in [
        SkillSkipReason::MissingManifest,
        SkillSkipReason::BadFrontMatter,
        SkillSkipReason::NameMismatch,
        SkillSkipReason::TooLarge,
        SkillSkipReason::Symlink,
        SkillSkipReason::NotUtf8,
        SkillSkipReason::Shadowed,
        SkillSkipReason::OverLimit,
        SkillSkipReason::Unreadable,
    ] {
        assert_eq!(serde_json::to_value(reason)?, json!(reason.code()));
    }
    for tier in [SkillTier::Install, SkillTier::User] {
        assert_eq!(serde_json::to_value(tier)?, json!(tier.label()));
    }
    Ok(())
}

#[test]
fn proto_u_011_the_report_is_bounded_into_a_valid_event() -> Result<(), Box<dyn Error>> {
    let many = MAX_CONTEXT_LOADED_ITEMS + 1;
    let report = ContextLoaded {
        instructions: vec![
            LoadedInstruction {
                path: "AGENTS.md".into(),
                truncated: false
            };
            many
        ],
        skills: vec![
            LoadedSkill {
                name: "a".into(),
                tier: SkillTier::User,
                description: "d".into()
            };
            many
        ],
        skipped: vec![
            SkippedSkill {
                folder: "f".into(),
                tier: SkillTier::Install,
                reason: SkillSkipReason::Symlink
            };
            many
        ],
    };
    let event = report.into_event();
    let Event::ContextLoaded {
        instructions,
        skills,
        skipped,
    } = &event
    else {
        return Err("expected context.loaded".into());
    };
    assert_eq!(
        (instructions.len(), skills.len(), skipped.len()),
        (
            MAX_CONTEXT_LOADED_ITEMS,
            MAX_CONTEXT_LOADED_ITEMS,
            MAX_CONTEXT_LOADED_ITEMS
        )
    );
    let encoded = frame(&serde_json::to_value(&event)?)?;
    assert!(
        ServerFrame::parse(&encoded).is_ok(),
        "a bounded report is a valid frame"
    );
    Ok(())
}
