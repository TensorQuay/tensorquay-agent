//! PROMPT-U-004 / CORE-U-022 / CORE-U-023 / CORE-U-054: the text protocol for tool calls, its repairs and bounds.
use proptest::prelude::*;
use serde_json::{Value, json};
use tqa_prompt::{
    FALLBACK_INSTRUCTIONS, FALLBACK_PROMPT_VERSION, FallbackCall, FallbackFailure, ToolCheck,
    fallback_instructions, parse_reply, render_call, streamable_len,
};

/// `read_file` needs a string `path`; `shell` needs a string `command`; nothing else exists.
fn check(name: &str, arguments: &Value) -> ToolCheck {
    let field = match name {
        "read_file" => "path",
        "shell" => "command",
        _ => return ToolCheck::Unknown,
    };
    if arguments.get(field).is_some_and(Value::is_string) {
        ToolCheck::Valid
    } else {
        ToolCheck::Invalid
    }
}
fn parse(text: &str) -> Result<(String, Vec<FallbackCall>), FallbackFailure> {
    parse_reply(text, true, &check).map(|reply| (reply.narration, reply.calls))
}
fn call(name: &str, arguments: Value) -> FallbackCall {
    FallbackCall {
        name: name.into(),
        arguments,
    }
}

// TEST: PROMPT-U-027
#[test]
fn prompt_u_004_instructions_only_for_routes_without_native_tools() {
    assert_eq!(fallback_instructions(true), None);
    assert_eq!(fallback_instructions(false), Some(FALLBACK_INSTRUCTIONS));
    // PROMPT-U-027: version 5, since the retry sentence a model sees names the problem (ADR-0007).
    assert_eq!(FALLBACK_PROMPT_VERSION, 5);
    assert!(FALLBACK_INSTRUCTIONS.contains("```tqa_call"));
    assert!(FALLBACK_INSTRUCTIONS.contains("data"));
}

#[test]
fn core_u_022_well_formed_blocks_parse_in_order_with_prose_as_narration() {
    let text = concat!(
        "I will read it.\n",
        "```tool_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}\n```\n",
        "Then run it.\n",
        "```tool_call\n{\"name\": \"shell\", \"arguments\": {\"command\": \"ls\"}}\n```\n"
    );
    assert_eq!(
        parse(text),
        Ok((
            "I will read it.\nThen run it.\n".into(),
            vec![
                call("read_file", json!({"path": "a.rs"})),
                call("shell", json!({"command": "ls"}))
            ]
        ))
    );
    assert_eq!(
        parse("Just an answer."),
        Ok(("Just an answer.".into(), vec![]))
    );
}

#[test]
fn core_u_022_each_repair_rule_recovers_one_common_fault() -> Result<(), String> {
    let expected = vec![call("read_file", json!({"path": "a.rs"}))];
    for (rule, text) in [
        (
            "1 trailing comma",
            "```tool_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\",},}\n```",
        ),
        (
            "2 unquoted keys and single quotes",
            "```tool_call\n{name: 'read_file', arguments: {path: 'a.rs'}}\n```",
        ),
        (
            "3 missing closing fence at the end",
            "```tool_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}",
        ),
        (
            "4 json label with exactly the two keys",
            "```json\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}\n```",
        ),
        (
            "4 unlabelled fence",
            "```\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}\n```",
        ),
        (
            "5 bare object",
            "  {\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}\n",
        ),
    ] {
        let (narration, calls) = parse(text).map_err(|error| format!("{rule}: {error:?}"))?;
        assert_eq!(calls, expected, "{rule}");
        assert!(narration.trim().is_empty(), "{rule}: {narration:?}");
    }
    Ok(())
}

#[test]
fn core_u_022_guarded_repairs_leave_ordinary_json_as_text() {
    for text in [
        "Example:\n```json\n{\"name\": \"alice\", \"arguments\": {\"x\": 1}}\n```\n",
        "```json\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a\"}, \"extra\": 1}\n```",
        "```rust\nfn main() {}\n```",
        "{\"answer\": 42}",
    ] {
        assert_eq!(parse(text), Ok((text.to_owned(), vec![])), "{text}");
    }
}

#[test]
fn core_u_022_attempts_that_cannot_be_read_are_typed_failures() {
    for (text, failure) in [
        (
            "```tool_call\n{\"name\": \"read_file\" \"arguments\": }\n```",
            FallbackFailure::Malformed,
        ),
        (
            "```tool_call\n{\"name\": \"deploy\", \"arguments\": {}}\n```",
            FallbackFailure::UnknownTool,
        ),
        (
            "```tool_call\n{\"name\": \"read_file\", \"arguments\": \"a.rs\"}\n```",
            FallbackFailure::Arguments,
        ),
        (
            "```tool_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": 7}}\n```",
            FallbackFailure::Schema,
        ),
        (
            "```json\n{\"name\": \"read_file\", \"arguments\": {\"path\": 7}}\n```",
            FallbackFailure::Schema,
        ),
        (
            "```tool_call\n{\"name\": \"read_file\"}\n```",
            FallbackFailure::Malformed,
        ),
    ] {
        assert_eq!(parse(text), Err(failure), "{text}");
    }
    let two = concat!(
        "```tool_call\n{\"name\": \"shell\", \"arguments\": {\"command\": \"a\"}}\n```\n",
        "```tool_call\n{\"name\": \"shell\", \"arguments\": {\"command\": \"b\"}}\n```\n"
    );
    assert_eq!(
        parse_reply(two, false, &check).err(),
        Some(FallbackFailure::TooMany)
    );
    assert_eq!(
        parse_reply(two, true, &check).map(|reply| reply.calls.len()),
        Ok(2)
    );
    for failure in [
        FallbackFailure::Malformed,
        FallbackFailure::UnknownTool,
        FallbackFailure::Arguments,
        FallbackFailure::Schema,
        FallbackFailure::TooMany,
    ] {
        assert!(!failure.code().is_empty() && failure.code().len() <= 16);
    }
}

#[test]
fn core_u_022_streamed_prefix_stops_before_any_possible_call() {
    let text = "Reading now.\n```tool_call\n{}\n```\nDone.";
    assert_eq!(streamable_len(text), "Reading now.\n".len());
    assert_eq!(streamable_len("Partly `"), "Partly `".len());
    assert_eq!(streamable_len("line\n`"), "line\n".len());
    assert_eq!(streamable_len("  {\"name\""), 0);
    assert_eq!(streamable_len("   "), 0);
    assert_eq!(streamable_len("An answer {with braces}"), 23);
}

fn arguments() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(Value::from),
        any::<i32>().prop_map(Value::from),
        "[ -~\\n`'\"{}\\\\é❤]{0,24}".prop_map(Value::from),
    ];
    let value = leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::from),
            prop::collection::btree_map("[a-z_]{1,8}", inner, 0..4)
                .prop_map(|map| Value::Object(map.into_iter().collect())),
        ]
    });
    prop::collection::btree_map("[a-z_]{1,8}", value, 0..4)
        .prop_map(|map| Value::Object(map.into_iter().collect()))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 1000, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn core_u_023_rendered_calls_parse_back_identically(
        name in "[a-z][a-z0-9_]{0,30}",
        arguments in arguments(),
        prose in "[ -~]{0,40}",
    ) {
        let text = format!("{}\n{}\n", prose.replace('`', "'"), render_call(&name, &arguments));
        let reply = parse_reply(&text, true, &|_: &str, _: &Value| ToolCheck::Valid);
        let reply = reply.map_err(|error| TestCaseError::fail(format!("{error:?}")))?;
        prop_assert_eq!(reply.calls, vec![FallbackCall { name, arguments }]);
    }
}

/// CORE-U-054: the instructions and rendered calls use `tqa_call` (instruction version 4, after a 20-per-arm A/B);
/// `tool_call` blocks (version 3) still read the same way, so replies and journals from then parse.
#[test]
fn core_u_054_the_instructions_and_rendered_calls_use_tqa_call()
-> Result<(), Box<dyn std::error::Error>> {
    assert!(
        !FALLBACK_INSTRUCTIONS.contains("tool_call"),
        "{FALLBACK_INSTRUCTIONS}"
    );
    let rendered = render_call("read_file", &json!({"path": "a.rs"}));
    assert!(rendered.starts_with("```tqa_call\n"), "{rendered}");
    // A rendered call reads back as the same call.
    assert_eq!(
        parse(&rendered)?,
        (
            String::new(),
            vec![call("read_file", json!({"path": "a.rs"}))]
        )
    );
    Ok(())
}

#[test]
fn core_u_054_the_old_label_reads_exactly_as_the_new_one() {
    let bodies = [
        // Well formed.
        "{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}",
        // Repaired (trailing commas).
        "{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\",},}",
        // Refused: unknown tool, invalid arguments, malformed.
        "{\"name\": \"deploy\", \"arguments\": {}}",
        "{\"name\": \"read_file\", \"arguments\": {\"path\": 7}}",
        "{\"name\": \"read_file\" \"arguments\": }",
    ];
    for body in bodies {
        let old = parse(&format!("Reading.\n```tool_call\n{body}\n```\n"));
        let new = parse(&format!("Reading.\n```tqa_call\n{body}\n```\n"));
        assert_eq!(old, new, "{body}");
    }
    // Both labels in one reply: two calls, in order.
    let mixed = concat!(
        "```tool_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.rs\"}}\n```\n",
        "```tqa_call\n{\"name\": \"shell\", \"arguments\": {\"command\": \"ls\"}}\n```\n",
    );
    assert_eq!(
        parse(mixed),
        Ok((
            String::new(),
            vec![
                call("read_file", json!({"path": "a.rs"})),
                call("shell", json!({"command": "ls"}))
            ]
        ))
    );
}

/// Every prefix of `text` streams no further than `limit` bytes.
fn streams_at_most(text: &str, limit: usize) {
    for end in (0..=text.len()).filter(|end| text.is_char_boundary(*end)) {
        assert!(
            streamable_len(&text[..end]) <= limit,
            "{:?} streamed {} bytes, more than {limit}",
            &text[..end],
            streamable_len(&text[..end])
        );
    }
}

// TEST: PROMPT-U-022
/// The survey's two text-route endings (13h and c14b-rc): closing tags of the native markup the model slipped into after
/// its sentence. They never stream, and the narration ends where they began; the sentence and the blank line stay.
#[test]
fn prompt_u_022_trailing_closing_tags_are_held_back_and_dropped() -> Result<(), FallbackFailure> {
    for (sentence, tags) in [
        (
            "I'll start by finding all occurrences of calc_total.\n\n",
            "</parameter>\n</tool_call>\n</parameter>\n</tool_call>",
        ),
        ("The tests pass now.\n", "</function>\n</tool_call>\n"),
        ("Done.\n", "</function-call>\n</function_call>"),
    ] {
        let text = format!("{sentence}{tags}");
        streams_at_most(&text, sentence.len());
        let (narration, calls) = parse(&text)?;
        assert_eq!(narration, sentence, "{text:?}");
        assert!(calls.is_empty());
    }
    // A reply of nothing but such tags shows nothing.
    assert_eq!(parse("</tool_call>\n")?.0, "");
    Ok(())
}

// TEST: PROMPT-U-022
/// Only a trailing run of lines that hold nothing but those closing tags goes: prose after them releases them, a tag
/// inside a sentence or a fenced block is the text's own, and an opening or unknown tag is never dropped.
#[test]
fn prompt_u_022_closing_tags_that_are_not_a_trailing_run_stay() -> Result<(), FallbackFailure> {
    let released = "First.\n</tool_call>\nThen more prose.\n";
    assert_eq!(streamable_len(released), released.len());
    assert_eq!(parse(released)?.0, released);
    for kept in [
        "End a call with </tool_call> on its own line.\n",
        "An example:\n```xml\n<tool_call>x</tool_call>\n</tool_call>\n```\n",
        "Done.\n<tool_call>\n",
        "Done.\n</div>\n",
    ] {
        assert_eq!(parse(kept)?.0, kept, "{kept:?}");
    }
    Ok(())
}

// TEST: PROMPT-U-023
#[test]
fn prompt_u_023_a_copied_correction_has_its_own_code() {
    // The web words this code for the retry's tooltip (WEB-U-304), so it is stable.
    assert_eq!(FallbackFailure::Echo.code(), "echo");
    assert_eq!(FallbackFailure::Echo.to_string(), "echo");
}
