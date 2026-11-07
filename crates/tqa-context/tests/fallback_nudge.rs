//! CORE-U-022: the retry prompt after an unreadable text-protocol reply is derived from the journal.
#[allow(
    dead_code,
    reason = "This suite uses only the frame builder from the shared vocabulary."
)]
#[path = "support/history.rs"]
mod support;
use std::error::Error;
use support::frames;
use tqa_context::{Role, project};
use tqa_types::Event;

fn history(reason: &str) -> Result<Vec<Event>, Box<dyn Error>> {
    Ok(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "task".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: "r".parse()?,
        },
        Event::StepStarted {
            step: 1,
            attempt: 1,
        },
        Event::TextDelta {
            text: "partial".into(),
        },
        Event::ProviderRetry {
            attempt: 1,
            delay_ms: 0,
            reason: reason.into(),
            provider_message: None,
            fallback_problem: None,
        },
        Event::StepStarted {
            step: 1,
            attempt: 2,
        },
    ])
}

#[test]
fn core_u_022_a_fallback_retry_adds_one_fixed_nudge_after_the_task() -> Result<(), Box<dyn Error>> {
    let messages = project(&frames(history("fallback_parse: unknown_tool")?))?;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].text, "task");
    assert_eq!(messages[1].role, Role::User);
    assert_eq!(
        messages[1].text,
        "[The last reply could not be read as a tool call: it names a tool that does not exist. \
         Reply again with each call in its own tqa_call block.]"
    );
    // The journal alone reproduces it, so a resumed attempt sends the same nudge.
    let replay: Vec<tqa_types::ServerFrame> = serde_json::from_slice(&serde_json::to_vec(
        &frames(history("fallback_parse: unknown_tool")?),
    )?)?;
    assert_eq!(project(&replay)?, messages);
    Ok(())
}

#[test]
fn core_u_022_every_reason_has_fixed_text_and_other_retries_add_nothing()
-> Result<(), Box<dyn Error>> {
    for (code, phrase) in [
        ("malformed", "the tqa_call block is not valid JSON"),
        ("unknown_tool", "it names a tool that does not exist"),
        ("arguments", "its arguments are not a JSON object"),
        ("schema", "its arguments do not match the tool's schema"),
        ("too_many", "this model route accepts one call per reply"),
        ("something_else", "the reply was not in the expected format"),
    ] {
        let messages = project(&frames(history(&format!("fallback_parse: {code}"))?))?;
        assert!(messages[1].text.contains(phrase), "{code}");
    }
    for reason in ["http_error 503", "empty_response", "fallback"] {
        assert_eq!(project(&frames(history(reason)?))?.len(), 1, "{reason}");
    }
    Ok(())
}
