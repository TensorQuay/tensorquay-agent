//! PROMPT-U-008 (Gate 5, 29 Sep): on a text-protocol route, a model sometimes writes its call in its native markup
//! instead of a fenced `tqa_call` block. Where the server has no tool-call parser of its own, that markup arrives as
//! text, and the parser recovers it: the XML form (`<function=…>` with `<parameter=…>` values, inside `<tool_call>` or
//! not) and the JSON form (`<tool_call>{"name", "arguments"}</tool_call>`). Only a known tool with valid arguments is a
//! call; anything else stays prose. A value is typed from JSON when the tool accepts it that way, and kept as text
//! otherwise. Fenced blocks win: markup is only read when the reply has no fenced call.
use serde_json::{Value, json};
use tqa_prompt::{FallbackCall, ToolCheck, parse_reply};

/// `read_file` needs a string `path`; `glob` a string `pattern` and, optionally, an integer `max_results`.
fn check(name: &str, arguments: &Value) -> ToolCheck {
    match name {
        "read_file" if arguments.get("path").is_some_and(Value::is_string) => ToolCheck::Valid,
        "glob"
            if arguments.get("pattern").is_some_and(Value::is_string)
                && arguments.get("max_results").is_none_or(Value::is_u64) =>
        {
            ToolCheck::Valid
        }
        "read_file" | "glob" => ToolCheck::Invalid,
        _ => ToolCheck::Unknown,
    }
}
fn calls(text: &str) -> Vec<FallbackCall> {
    parse_reply(text, true, &check)
        .map(|reply| reply.calls)
        .unwrap_or_default()
}
fn call(name: &str, arguments: Value) -> FallbackCall {
    FallbackCall {
        name: name.into(),
        arguments,
    }
}

// TEST: PROMPT-U-008
#[test]
fn prompt_u_008_native_xml_and_json_calls_are_recovered() -> Result<(), tqa_prompt::FallbackFailure>
{
    let xml = "I'll read it.\n<tool_call>\n<function=read_file>\n<parameter=path>\nsrc/main.rs\n</parameter>\n\
               </function>\n</tool_call>";
    let reply = parse_reply(xml, true, &check)?;
    assert_eq!(
        reply.calls,
        [call("read_file", json!({"path": "src/main.rs"}))]
    );
    assert_eq!(
        reply.narration.trim(),
        "I'll read it.",
        "the markup leaves the narration"
    );
    // Without the wrapper, and a value typed from JSON because the tool takes an integer there.
    assert_eq!(
        calls(
            "<function=glob>\n<parameter=pattern>\n*.md\n</parameter>\n<parameter=max_results>\n5\n</parameter>\n</function>"
        ),
        [call("glob", json!({"pattern": "*.md", "max_results": 5}))]
    );
    // A value that reads as JSON but the tool wants as text stays text.
    assert_eq!(
        calls("<function=read_file>\n<parameter=path>\n123\n</parameter>\n</function>"),
        [call("read_file", json!({"path": "123"}))]
    );
    // The JSON form, and two calls in one reply.
    assert_eq!(
        calls(
            "<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a\"}}\n</tool_call>\n\
               <tool_call>\n{\"name\": \"glob\", \"arguments\": {\"pattern\": \"*\"}}\n</tool_call>"
        ),
        [
            call("read_file", json!({"path": "a"})),
            call("glob", json!({"pattern": "*"}))
        ]
    );
    Ok(())
}

// TEST: PROMPT-U-008
#[test]
fn prompt_u_008_unknown_or_invalid_markup_stays_prose_and_fenced_blocks_win()
-> Result<(), tqa_prompt::FallbackFailure> {
    for text in [
        "<function=delete_everything>\n<parameter=path>\n/\n</parameter>\n</function>",
        "<function=read_file>\n<parameter=nothing>\nx\n</parameter>\n</function>",
        "<tool_call>\nnot json\n</tool_call>",
        "The <function=...> tag is how some models call tools.",
    ] {
        let reply = parse_reply(text, true, &check)?;
        assert!(reply.calls.is_empty(), "{text}");
        assert_eq!(reply.narration, text, "kept as it was");
    }
    let mixed = "```tqa_call\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a\"}}\n```\n\
                 <function=read_file>\n<parameter=path>\nb\n</parameter>\n</function>";
    assert_eq!(
        calls(mixed),
        [call("read_file", json!({"path": "a"}))],
        "the fenced call only"
    );
    Ok(())
}

// TEST: PROMPT-U-008
#[test]
fn prompt_u_008_values_holding_markup_fenced_markup_and_markup_before_prose_stay_prose()
-> Result<(), tqa_prompt::FallbackFailure> {
    for text in [
        // A value that holds the markup itself would be cut at its first closing tag: refused, never a shorter call.
        "<function=read_file>\n<parameter=path>\nnotes/</parameter>.md\n</parameter>\n</function>",
        "<function=read_file>\n<parameter=path>\na<parameter=b\n</parameter>\n</function>",
        // A closing tag left over after the piece.
        "<function=read_file>\n<parameter=path>\na\n</parameter>\n</function>\n</parameter>",
        // Markup inside a fenced block is an example, not a call.
        "Tools are called like this:\n```xml\n<function=read_file>\n<parameter=path>\na\n</parameter>\n</function>\n```",
        // Only a trailing run of markup is read: here prose follows it.
        "<function=read_file>\n<parameter=path>\na\n</parameter>\n</function>\nThat is how you call it.",
    ] {
        let reply = parse_reply(text, true, &check)?;
        assert!(reply.calls.is_empty(), "{text}");
        assert_eq!(reply.narration, text, "kept as it was");
    }
    Ok(())
}
