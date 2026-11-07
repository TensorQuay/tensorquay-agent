//! PROMPT-U-010: on a native-tools route, a model sometimes writes its call as text instead of calling the tool (qwen3:4b
//! on Ollama, 30 Sep: `<function-call>` around the call's JSON). `parse_text_calls` reads such a reply: a readable call to
//! a known tool with valid arguments comes back to run; a reply that plainly ends in call markup it can't read is a
//! `text_call` failure, so a run never ends as if the call had happened; a plain answer, markup quoted in the middle of
//! prose and markup shown in a fenced example are answers, with no call.
use serde_json::{Value, json};
use tqa_prompt::{FallbackCall, FallbackFailure, ToolCheck, parse_text_calls};

/// `write_file` needs a string `path` and `content`, and takes an optional boolean `create_parents`.
fn check(name: &str, arguments: &Value) -> ToolCheck {
    match name {
        "write_file"
            if arguments.get("path").is_some_and(Value::is_string)
                && arguments.get("content").is_some_and(Value::is_string)
                && arguments
                    .get("create_parents")
                    .is_none_or(Value::is_boolean) =>
        {
            ToolCheck::Valid
        }
        "write_file" => ToolCheck::Invalid,
        _ => ToolCheck::Unknown,
    }
}

fn write(path: &str, content: &str) -> FallbackCall {
    FallbackCall {
        name: "write_file".into(),
        arguments: json!({"path": path, "content": content, "create_parents": false}),
    }
}

/// The shape a local qwen3 run ended with: the call's JSON, its arguments nested one level, then `</function-call>`.
fn qwen(open: &str, close: &str, body: &str) -> String {
    format!("I'll write the explanation to notes.md.\n\n{open}\n{body}\n{close}")
}
const BODY: &str = "{\n  \"name\": \"write_file\",\n  \"arguments\": {\n    \"path\": \"notes.md\",\n    \
                    \"content\": \"Photosynthesis turns light into sugar.\",\n    \"create_parents\": false\n  }\n}";

// TEST: PROMPT-U-010
#[test]
fn prompt_u_010_a_call_written_as_text_is_read_to_run() -> Result<(), FallbackFailure> {
    let expected = [write("notes.md", "Photosynthesis turns light into sugar.")];
    for (open, close) in [
        ("<function-call>", "</function-call>"),
        ("<function_call>", "</function_call>"),
        ("<tool_call>", "</tool_call>"),
    ] {
        assert_eq!(
            parse_text_calls(&qwen(open, close, BODY), true, &check)?,
            expected,
            "{open}"
        );
    }
    // A tqa_call block, the text protocol's own form.
    let fenced = format!("Writing it now.\n```tqa_call\n{BODY}\n```\n");
    assert_eq!(parse_text_calls(&fenced, true, &check)?, expected);
    Ok(())
}

// TEST: PROMPT-U-010
#[test]
fn prompt_u_010_answers_and_quoted_markup_have_no_call() -> Result<(), FallbackFailure> {
    for answer in [
        "Photosynthesis turns light into sugar.",
        // Markup named in the middle of prose, with prose after it.
        "Some models write <function-call> tags instead of calling a tool; this one didn't.",
        // Markup shown as an example in a fence.
        "An example:\n```xml\n<function-call>\n{\"name\": \"x\"}\n</function-call>\n```\nThat is all.",
    ] {
        assert_eq!(parse_text_calls(answer, true, &check)?, [], "{answer}");
    }
    Ok(())
}

// TEST: PROMPT-U-010
#[test]
fn prompt_u_010_markup_that_cant_be_read_is_a_text_call() {
    let broken = BODY.replace(
        "\"create_parents\": false\n  }",
        "\"create_parents\": false\n  ",
    );
    for reply in [
        // The call's JSON cut short, still closed by the tag.
        qwen("<function-call>", "</function-call>", &broken),
        // The closing tag alone at the end, its opening not written.
        format!("Here it is:\n{BODY}\n</function-call>"),
        // A tool that doesn't exist, and arguments the tool refuses.
        qwen(
            "<tool_call>",
            "</tool_call>",
            "{\"name\": \"save_file\", \"arguments\": {\"path\": \"a\"}}",
        ),
        qwen(
            "<function-call>",
            "</function-call>",
            "{\"name\": \"write_file\", \"arguments\": {\"path\": 7}}",
        ),
        // An opening tag starting a line, with nothing readable after it.
        "Writing it.\n<function-call>\n{\"name\": \"write_file\"".to_owned(),
        // A text-protocol block this route doesn't use, unreadable.
        "```tqa_call\n{\"name\": \"write_file\",\n```".to_owned(),
    ] {
        assert_eq!(
            parse_text_calls(&reply, true, &check),
            Err(FallbackFailure::TextCall),
            "{reply}"
        );
    }
    assert_eq!(FallbackFailure::TextCall.code(), "text_call");
}

/// qwen3:4b on Ollama, 30 Sep, captured whole: a native route's reply that was only the call, as JSON, under `function`.
const CAPTURED: &str = include_str!("fixtures/qwen3-4b-bare-function-call.txt");

// TEST: PROMPT-U-010
#[test]
fn prompt_u_010_a_whole_reply_that_is_one_call_object_runs() -> Result<(), FallbackFailure> {
    let calls = parse_text_calls(CAPTURED, true, &check)?;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "write_file");
    assert_eq!(calls[0].arguments["path"], "notes.md");
    assert!(
        calls[0].arguments["content"]
            .as_str()
            .is_some_and(|content| content.starts_with("Photosynthesis is the vital"))
    );
    // The other spellings of the same object: name with parameters, and tool.
    for reply in [
        "{\"name\": \"write_file\", \"parameters\": {\"path\": \"a.md\", \"content\": \"x\"}}",
        "  {\"tool\": \"write_file\", \"arguments\": {\"path\": \"a.md\", \"content\": \"x\"}}\n",
    ] {
        assert_eq!(
            parse_text_calls(reply, true, &check)?,
            [FallbackCall {
                name: "write_file".into(),
                arguments: json!({"path": "a.md", "content": "x"}),
            }],
            "{reply}"
        );
    }
    Ok(())
}

// TEST: PROMPT-U-010
#[test]
fn prompt_u_010_a_call_object_for_an_offered_tool_that_cant_run_is_a_text_call() {
    for reply in [
        // Arguments the tool refuses, no arguments, and an extra key beside the call's two.
        "{\"function\": \"write_file\", \"arguments\": {\"path\": 7}}",
        "{\"function\": \"write_file\"}",
        "{\"function\": \"write_file\", \"arguments\": {\"path\": \"a\", \"content\": \"x\"}, \"why\": \"notes\"}",
    ] {
        assert_eq!(
            parse_text_calls(reply, true, &check),
            Err(FallbackFailure::TextCall),
            "{reply}"
        );
    }
}

// TEST: PROMPT-U-010
#[test]
fn prompt_u_010_json_that_only_mentions_a_tool_is_an_answer() -> Result<(), FallbackFailure> {
    for answer in [
        // A person asked for JSON: it names a tool inside a longer answer, fenced or inline.
        "Here is the settings file you asked for:\n```json\n{\"tool\": \"write_file\", \"enabled\": true}\n```\nPut it next to the others.",
        "A call looks like {\"function\": \"write_file\", \"arguments\": {\"path\": \"a.md\", \"content\": \"x\"}}, but I won't make one.",
        // A json block that is exactly a valid call, inside an answer: an example, not a call, on a native route.
        "An example call:\n```json\n{\"name\": \"write_file\", \"arguments\": {\"path\": \"a.md\", \"content\": \"x\"}}\n```\nThat's all.",
        // A whole-reply object whose name isn't a tool this run offers: just JSON.
        "{\"name\": \"Alice\", \"arguments\": {\"age\": 3}}",
        "{\"function\": \"save_file\", \"arguments\": {\"path\": \"a.md\"}}",
        "{\"theme\": \"dark\", \"size\": 14}",
    ] {
        assert_eq!(parse_text_calls(answer, true, &check)?, [], "{answer}");
    }
    Ok(())
}
