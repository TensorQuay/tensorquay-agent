//! CTX-U-018: the runtime's own correction sentence, recognised when a model copies it back as its whole reply (the text
//! route's survey of 7 Oct 2026: one c14b-rc reply was nothing but it, shown as the agent's message).
use tqa_context::{CORRECTION_PREFIX, is_correction};

// TEST: CTX-U-018
#[test]
fn ctx_u_018_a_correction_copied_back_whole_is_recognised() {
    for sentence in [
        "[The last reply could not be read as a tool call: this model route accepts one call per reply. Reply again \
         with each call in its own tqa_call block.]",
        "[The last reply could not be read as a tool call: the tqa_call block is not valid JSON. Reply again with each \
         call in its own tqa_call block.]",
        "[The last reply could not be read as a tool call: the reply was not in the expected format. Reply again with \
         each call in its own tqa_call block.]",
        "[The last reply wrote a tool call as text, so nothing ran. Call the tool through the tool interface instead of \
         writing it in the reply.]",
    ] {
        assert!(sentence.starts_with(CORRECTION_PREFIX));
        assert!(is_correction(sentence), "{sentence}");
        assert!(
            is_correction(&format!("\n  {sentence}\n")),
            "spaces around it"
        );
    }
}

// TEST: CTX-U-018
#[test]
fn ctx_u_018_anything_more_or_else_is_the_models_own() {
    let sentence = "[The last reply could not be read as a tool call: this model route accepts one call per reply. \
                    Reply again with each call in its own tqa_call block.]";
    for text in [
        format!("{sentence}\nUnderstood, here is the call."),
        format!("Noted: {sentence}"),
        "[The last reply was fine.]".to_owned(),
        "The last reply could not be read as a tool call.".to_owned(),
        String::new(),
    ] {
        assert!(!is_correction(&text), "{text}");
    }
}

// TEST: CTX-U-018
#[test]
fn ctx_u_018_a_reply_may_be_a_correction_until_it_parts_from_every_sentence() {
    for text in [
        "",
        "  ",
        "[",
        "[The last",
        "[The last reply could not be read",
        "[The last reply wrote a tool",
    ] {
        assert!(tqa_context::may_be_correction(text), "{text:?}");
    }
    for text in [
        "The",
        "[The lost",
        "[The last reply was fine",
        "Sure. [The last reply",
    ] {
        assert!(!tqa_context::may_be_correction(text), "{text:?}");
    }
}

// TEST: CTX-U-019
#[test]
fn ctx_u_019_a_leading_correction_is_measured_with_the_space_after_it() {
    let sentence = "[The last reply wrote a tool call as text, so nothing ran. Call the tool through the tool interface \
                    instead of writing it in the reply.]";
    let reply = format!("\n {sentence}\n\nReading it.\n");
    assert_eq!(
        tqa_context::leading_correction(&reply),
        Some(reply.len() - "Reading it.\n".len())
    );
    assert_eq!(
        tqa_context::leading_correction(sentence),
        Some(sentence.len())
    );
    for text in [
        format!("Noted: {sentence}"),
        sentence.trim_end_matches(']').to_owned(),
        "[The last reply was fine.] Reading it.".to_owned(),
        String::new(),
    ] {
        assert_eq!(tqa_context::leading_correction(&text), None, "{text}");
    }
}
