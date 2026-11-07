//! PROMPT-U-016 (F25, candidate 12: qwen2.5 wrote its call in a `json` fence and the run ended "Done" having done
//! nothing): on a native-tools route, a `json` fence counts as a call only when it ends the reply and its whole content
//! is one call object (`name`, `function` or `tool`, with `arguments` or `parameters`) naming an offered tool whose
//! arguments validate. Any other `json` fence, and JSON a person asked for, stays the answer.
use serde_json::{Value, json};
use tqa_prompt::{FallbackCall, ToolCheck, parse_text_calls, parse_text_reply};

fn check(name: &str, arguments: &Value) -> ToolCheck {
    match name {
        "write_notes" if arguments.get("text").is_some_and(Value::is_string) => ToolCheck::Valid,
        "write_notes" => ToolCheck::Invalid,
        _ => ToolCheck::Unknown,
    }
}

const CALL: &str = "{\"name\": \"write_notes\", \"arguments\": {\"text\": \"Q3 is provisional.\"}}";

// TEST: PROMPT-U-016
#[test]
fn prompt_u_016_a_trailing_json_fenced_call_runs() -> Result<(), Box<dyn std::error::Error>> {
    let expected = vec![FallbackCall {
        name: "write_notes".into(),
        arguments: json!({"text": "Q3 is provisional."}),
    }];
    for reply in [
        format!("I'll save the note.\n```json\n{CALL}\n```"),
        format!("```json\n{CALL}\n```\n\n"),
        format!("I'll save the note.\n```JSON\n{CALL}\n```"),
        "Saving.\n```json\n{\"function\": \"write_notes\", \"parameters\": {\"text\": \"Q3 is provisional.\"}}\n```"
            .to_owned(),
    ] {
        assert_eq!(parse_text_calls(&reply, true, &check)?, expected, "{reply:?}");
    }
    // Only the narration is left of it.
    let reply = parse_text_reply(
        &format!("I'll save the note.\n```json\n{CALL}\n```"),
        true,
        &check,
    )?;
    assert_eq!(reply.narration, "I'll save the note.\n");
    Ok(())
}

// TEST: PROMPT-U-016
#[test]
fn prompt_u_016_any_other_json_fence_stays_the_answer() -> Result<(), Box<dyn std::error::Error>> {
    for reply in [
        // A JSON answer about tools, as a person may ask for.
        "Here are the tools:\n```json\n{\"tools\": [\"write_notes\", \"read_file\"], \"count\": 2}\n```".to_owned(),
        // An example of a call, explained after it.
        format!("Here is how a call would look:\n```json\n{CALL}\n```\nThat's the shape the tool takes."),
        // More than a call object, an unknown tool, arguments the tool refuses, an unclosed fence.
        "```json\n{\"name\": \"write_notes\", \"arguments\": {\"text\": \"x\"}, \"id\": 3}\n```".to_owned(),
        "```json\n{\"name\": \"delete_everything\", \"arguments\": {}}\n```".to_owned(),
        "```json\n{\"name\": \"write_notes\", \"arguments\": {\"text\": 7}}\n```".to_owned(),
        format!("```json\n{CALL}"),
        // Two objects, or an array.
        format!("```json\n[{CALL}]\n```"),
    ] {
        let reply_text = reply.clone();
        let read = parse_text_reply(&reply, true, &check)?;
        assert!(read.calls.is_empty(), "{reply_text:?}");
        assert_eq!(read.narration, reply_text, "the answer is left whole");
    }
    Ok(())
}
