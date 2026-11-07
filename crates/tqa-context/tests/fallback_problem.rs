//! CTX-U-020 to CTX-U-022: the retry sentence after an unreadable text-protocol reply names what was wrong, from the
//! `provider.retry` event's `fallback_problem` (ADR-0007); without one it is today's sentence, byte for byte; and a
//! model that copies a detailed sentence back is still caught.
#[allow(
    dead_code,
    reason = "This suite uses only the frame builder from the shared vocabulary."
)]
#[path = "support/history.rs"]
mod support;
use serde_json::json;
use std::error::Error;
use support::frames;
use tqa_context::{is_correction, leading_correction, may_be_correction, project};
use tqa_types::{Event, FallbackProblem};

type TestResult = Result<(), Box<dyn Error>>;

fn history(reason: &str, problem: Option<FallbackProblem>) -> Result<Vec<Event>, Box<dyn Error>> {
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
        Event::ProviderRetry {
            attempt: 1,
            delay_ms: 0,
            reason: reason.into(),
            provider_message: None,
            fallback_problem: problem,
        },
        Event::StepStarted {
            step: 1,
            attempt: 2,
        },
    ])
}
fn sentence(code: &str, problem: Option<FallbackProblem>) -> Result<String, Box<dyn Error>> {
    let messages = project(&frames(history(
        &format!("fallback_parse: {code}"),
        problem,
    )?))?;
    assert_eq!(messages.len(), 2, "the task and the sentence");
    Ok(messages[1].text.clone())
}
fn problem(value: serde_json::Value) -> Result<FallbackProblem, Box<dyn Error>> {
    Ok(serde_json::from_value(value)?)
}

/// Each kind's sentence, written out (CTX-U-020): the failure code, the problem as JSON, and the sentence.
const CASES: &[(&str, &str, &str)] = &[
    (
        "malformed",
        r#"{"kind":"no_name"}"#,
        "[The last reply could not be read as a tool call: the tqa_call block has no \"name\" key. Each block holds one object with exactly two keys: \"name\", the tool, and \"arguments\". Reply again with each call in its own tqa_call block.]",
    ),
    (
        "malformed",
        r#"{"kind":"no_arguments"}"#,
        "[The last reply could not be read as a tool call: the tqa_call block has no \"arguments\" key. Each block holds one object with exactly two keys: \"name\", the tool, and \"arguments\". Reply again with each call in its own tqa_call block.]",
    ),
    (
        "malformed",
        r#"{"kind":"not_json","cut_off":true,"line":1,"column":96}"#,
        "[The last reply could not be read as a tool call: the tqa_call block's JSON ends before the object closes. In long strings, write every quote as \\\" and every newline as \\n. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "malformed",
        r#"{"kind":"not_json","cut_off":false,"line":3,"column":27}"#,
        "[The last reply could not be read as a tool call: the tqa_call block is not valid JSON (line 3, column 27). In long strings, write every quote as \\\" and every newline as \\n. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "malformed",
        r#"{"kind":"not_object"}"#,
        "[The last reply could not be read as a tool call: the tqa_call block holds JSON that is not an object. Each block holds one object with exactly two keys: \"name\", the tool, and \"arguments\". Reply again with each call in its own tqa_call block.]",
    ),
    (
        "malformed",
        r#"{"kind":"extra_keys","keys":["tool","path"]}"#,
        "[The last reply could not be read as a tool call: the tqa_call block has keys other than \"name\" and \"arguments\" (\"tool\", \"path\"). Put the tool's parameters inside \"arguments\". Reply again with each call in its own tqa_call block.]",
    ),
    (
        "malformed",
        r#"{"kind":"extra_keys","keys":[]}"#,
        "[The last reply could not be read as a tool call: the tqa_call block has keys other than \"name\" and \"arguments\". Put the tool's parameters inside \"arguments\". Reply again with each call in its own tqa_call block.]",
    ),
    (
        "malformed",
        r#"{"kind":"name_not_string"}"#,
        "[The last reply could not be read as a tool call: its \"name\" is not a string. Each block holds one object with exactly two keys: \"name\", the tool, and \"arguments\". Reply again with each call in its own tqa_call block.]",
    ),
    (
        "unknown_tool",
        r#"{"kind":"unknown_tool","name":"search"}"#,
        "[The last reply could not be read as a tool call: it names a tool that does not exist (\"search\"). Reply again with each call in its own tqa_call block.]",
    ),
    (
        "unknown_tool",
        r#"{"kind":"unknown_tool"}"#,
        "[The last reply could not be read as a tool call: it names a tool that does not exist. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "schema",
        r#"{"kind":"unexpected_property","tool":"glob","property":"path","allowed":["cursor","max_results","pattern"]}"#,
        "[The last reply could not be read as a tool call: glob has no parameter \"path\". Its parameters are cursor, max_results and pattern. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "schema",
        r#"{"kind":"unexpected_property","tool":"glob","allowed":["pattern"]}"#,
        "[The last reply could not be read as a tool call: glob has an unknown parameter. Its parameters are pattern. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "schema",
        r#"{"kind":"missing_property","tool":"write_file","property":"create_parents","expected":["boolean"]}"#,
        "[The last reply could not be read as a tool call: write_file needs \"create_parents\" (a boolean). Add it to the arguments. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "schema",
        r#"{"kind":"wrong_type","tool":"write_file","property":"create_parents","expected":["boolean"]}"#,
        "[The last reply could not be read as a tool call: write_file's \"create_parents\" must be a boolean. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "schema",
        r#"{"kind":"wrong_type","tool":"glob","property":"max_results","expected":["integer","null"]}"#,
        "[The last reply could not be read as a tool call: glob's \"max_results\" must be an integer or null. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "schema",
        r#"{"kind":"missing_property","tool":"edit_file","at":"edits[0]","property":"new_text","expected":["string"]}"#,
        "[The last reply could not be read as a tool call: edit_file's edits[0] needs \"new_text\" (a string). Add it there. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "schema",
        r#"{"kind":"unexpected_property","tool":"edit_file","at":"edits[2]","property":"x","allowed":["new_text","old_text","replace_all"]}"#,
        "[The last reply could not be read as a tool call: edit_file's edits[2] has no \"x\". It takes new_text, old_text and replace_all. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "schema",
        r#"{"kind":"unexpected_property","tool":"edit_file","at":"edits[2]","allowed":[]}"#,
        "[The last reply could not be read as a tool call: edit_file's edits[2] has an unknown key. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "schema",
        r#"{"kind":"wrong_type","tool":"edit_file","at":"edits[0]","property":"replace_all","expected":["boolean","null"]}"#,
        "[The last reply could not be read as a tool call: edit_file's \"replace_all\" in edits[0] must be a boolean or null. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "schema",
        r#"{"kind":"other_schema","tool":"write_file"}"#,
        "[The last reply could not be read as a tool call: its arguments do not match the schema of write_file. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "too_many",
        r#"{"kind":"too_many","count":3,"limit":1}"#,
        "[The last reply could not be read as a tool call: this model route accepts one call per reply, and the reply had 3. Make one call now; the others can follow. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "too_many",
        r#"{"kind":"too_many","count":65,"limit":64}"#,
        "[The last reply could not be read as a tool call: this model route accepts at most 64 calls per reply, and the reply had 65. Reply again with each call in its own tqa_call block.]",
    ),
    (
        "malformed",
        r#"{"kind":"from_a_newer_runtime"}"#,
        "[The last reply could not be read as a tool call: the tqa_call block is not valid JSON. Reply again with each call in its own tqa_call block.]",
    ),
];

// TEST: CTX-U-020
#[test]
fn ctx_u_020_each_problem_has_its_sentence() -> TestResult {
    for &(code, value, expected) in CASES {
        let problem = problem(serde_json::from_str(value)?)?;
        assert_eq!(
            sentence(code, Some(problem.clone()))?,
            expected,
            "{problem:?}"
        );
        assert!(expected.chars().count() <= 400);
    }
    Ok(())
}

// TEST: CTX-U-020
#[test]
fn ctx_u_020_a_long_list_is_cut_to_stay_within_400_characters() -> TestResult {
    let long: Vec<String> = ('a'..='h')
        .map(|letter| format!("{letter}{}", "p".repeat(60)))
        .collect();
    let unexpected = problem(
        json!({"kind":"unexpected_property","tool":"t".repeat(64),"property":"x".repeat(64),"allowed":long}),
    )?;
    let text = sentence("schema", Some(unexpected))?;
    assert!(
        text.chars().count() <= 400,
        "{} characters: {text}",
        text.chars().count()
    );
    assert!(text.contains('…'), "{text}");
    assert!(is_correction(&text), "{text}");
    // A block's extra keys are cut the same way.
    let keys: Vec<String> = ('a'..='h')
        .map(|letter| format!("{letter}{}", "k".repeat(63)))
        .collect();
    let text = sentence(
        "malformed",
        Some(problem(json!({"kind":"extra_keys","keys":keys}))?),
    )?;
    assert!(
        text.chars().count() <= 400,
        "{} characters: {text}",
        text.chars().count()
    );
    assert!(text.contains("…)"), "{text}");
    assert!(is_correction(&text), "{text}");
    Ok(())
}

// TEST: CTX-U-020
#[test]
fn ctx_u_020_a_place_too_deep_to_fit_says_the_schema_in_general() -> TestResult {
    let name = |letter: char| format!("{letter}{}", "n".repeat(63));
    let at = format!("{}.{}.{}.{}", name('a'), name('b'), name('c'), name('d'));
    let tool = "t".repeat(64);
    let text = sentence(
        "schema",
        Some(problem(
            json!({"kind":"missing_property","tool":tool,"at":at,"property":name('p'),"expected":["string"]}),
        )?),
    )?;
    assert_eq!(
        text,
        format!(
            "[The last reply could not be read as a tool call: its arguments do not match the schema of {tool}. Reply again with each call in its own tqa_call block.]"
        )
    );
    Ok(())
}

// TEST: CTX-U-021
#[test]
fn ctx_u_021_without_a_problem_the_sentence_is_todays_byte_for_byte() -> TestResult {
    for (code, phrase) in [
        ("malformed", "the tqa_call block is not valid JSON"),
        ("unknown_tool", "it names a tool that does not exist"),
        ("arguments", "its arguments are not a JSON object"),
        ("schema", "its arguments do not match the tool's schema"),
        ("too_many", "this model route accepts one call per reply"),
        ("echo", "the reply was not in the expected format"),
    ] {
        assert_eq!(
            sentence(code, None)?,
            format!(
                "[The last reply could not be read as a tool call: {phrase}. Reply again with each call in its own tqa_call block.]"
            ),
            "{code}"
        );
    }
    assert_eq!(
        sentence("text_call", None)?,
        "[The last reply wrote a tool call as text, so nothing ran. Call the tool through the tool interface instead of \
         writing it in the reply.]"
    );
    Ok(())
}

// TEST: CTX-U-021
#[test]
fn ctx_u_021_the_journal_alone_reproduces_the_detailed_sentence() -> TestResult {
    for &(code, value, _) in CASES {
        let problem = problem(serde_json::from_str(value)?)?;
        let events = frames(history(&format!("fallback_parse: {code}"), Some(problem))?);
        let replay: Vec<tqa_types::ServerFrame> =
            serde_json::from_slice(&serde_json::to_vec(&events)?)?;
        assert_eq!(project(&replay)?, project(&events)?);
    }
    Ok(())
}

// TEST: CTX-U-022
#[test]
fn ctx_u_022_a_detailed_sentence_copied_back_is_recognised() -> TestResult {
    for &(code, value, expected) in CASES {
        let problem = problem(serde_json::from_str(value)?)?;
        let text = sentence(code, Some(problem))?;
        assert_eq!(text, expected);
        assert!(is_correction(&text), "{text}");
        assert!(is_correction(&format!("\n  {text}\n")), "spaces around it");
        assert!(
            !is_correction(&format!("{text}\nUnderstood, here is the call.")),
            "prose after it"
        );
        assert!(!is_correction(&format!("Noted: {text}")), "prose before it");
        assert!(!is_correction(&format!("{text} {text}")), "two of them");
        // Still arriving: it may be one all the way to its closing bracket.
        for end in text.char_indices().map(|(at, _)| at) {
            assert!(may_be_correction(&text[..end]), "{:?}", &text[..end]);
        }
        let reply = format!(" {text}\n\n```tqa_call\n{{}}\n```");
        assert_eq!(
            leading_correction(&reply),
            Some(reply.len() - "```tqa_call\n{}\n```".len())
        );
    }
    let opening =
        "[The last reply could not be read as a tool call: glob has no parameter \"path\".";
    for text in [
        format!("{opening}\nsomething else"),
        format!("{opening}] and then"),
        format!("{opening} {}", "x".repeat(400)),
    ] {
        assert!(!may_be_correction(&text), "{text}");
        assert!(!is_correction(&text), "{text}");
    }
    // A sentence broken across lines, or past 400 characters before its close, is not one leading the reply.
    let broken = "[The last reply could not be read as a tool call: glob has no parameter \"path\".\nReply again with each call in its own tqa_call block.] ok";
    assert_eq!(leading_correction(broken), None);
    let long = format!(
        "[The last reply could not be read as a tool call: {}. Reply again with each call in its own tqa_call block.] ok",
        "x".repeat(400)
    );
    assert_eq!(leading_correction(&long), None);
    Ok(())
}
