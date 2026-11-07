//! TK-U-037: the standalone fake model answers the gateway's route check itself (its fixed conversation, whose only
//! tool is `probe_echo` or `probe_lines`), with the check's own nonce, before any scenario selection: a check never
//! uses up an arrival-order reply and is never refused under prompt selection. So a stack's route check says
//! Verified. The gateway's side runs the real check against it (tqa-gateway `relay/probe_fake_model_tests.rs`).
use super::support::*;
use serde_json::{Value, json};

const NONCE: &str = "a1b2c3d4";

fn tool(name: &str) -> Value {
    json!({"type": "function", "function": {"name": name, "description": "check", "parameters": {"type": "object"}}})
}

fn check(user: &str, tools: &[&str]) -> Value {
    let mut body = json!({"model": "fixture", "stream": true, "messages": [
        {"role": "system", "content": "You are a connectivity test. Call the tool exactly once."},
        {"role": "user", "content": user}]});
    if !tools.is_empty() {
        body["tools"] = tools.iter().map(|name| tool(name)).collect();
    }
    body
}

fn echo() -> String {
    format!("Call probe_echo with value {NONCE}.")
}

/// The single tool call a stream makes, as (name, parsed arguments).
fn call(wire: &str) -> Result<(String, Value), Box<dyn std::error::Error>> {
    let mut name = String::new();
    let mut arguments = String::new();
    for chunk in chunks(wire)? {
        for delta in chunk["choices"][0]["delta"]["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
        {
            name.push_str(delta["function"]["name"].as_str().unwrap_or_default());
            arguments.push_str(delta["function"]["arguments"].as_str().unwrap_or_default());
        }
    }
    Ok((name, serde_json::from_str(&arguments)?))
}

fn text(wire: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(chunks(wire)?
        .iter()
        .filter_map(|chunk| {
            chunk["choices"][0]["delta"]["content"]
                .as_str()
                .map(str::to_owned)
        })
        .collect())
}

fn scenario(select: &str, when: Option<&str>) -> String {
    let mut reply = json!({"events": [{"type": "text", "text": "scripted-0"}],
                           "usage": {"prompt_tokens": 1, "completion_tokens": 1}});
    if let Some(when) = when {
        reply["when"] = json!(when);
    }
    json!({"model": "fixture", "select": select, "responses": [reply,
        {"events": [{"type": "text", "text": "scripted-1"}], "usage": {"prompt_tokens": 1, "completion_tokens": 1},
         "when": when}]})
    .to_string()
}

#[tokio::test]
async fn tk_u_037_the_check_is_answered_with_its_nonce_and_uses_no_arrival_reply() -> TestResult {
    let server = Binary::start(&scenario("arrival", None)).await?;
    // The native check: its tool with the nonce, then text after the tool's result.
    let (name, arguments) = call(&server.complete(check(&echo(), &["probe_echo"])).await?)?;
    assert_eq!(
        (name.as_str(), arguments),
        ("probe_echo", json!({"value": NONCE}))
    );
    let mut round_trip = check(&echo(), &["probe_echo"]);
    let messages = round_trip["messages"].as_array_mut().ok_or("messages")?;
    messages.push(json!({"role": "assistant", "content": null, "tool_calls": [{"id": "call_0", "type": "function",
        "function": {"name": "probe_echo", "arguments": json!({"value": NONCE}).to_string()}}]}));
    messages
        .push(json!({"role": "tool", "tool_call_id": "call_0", "content": format!("ok {NONCE}")}));
    let answer = server.complete(round_trip).await?;
    assert!(!text(&answer)?.trim().is_empty(), "{answer}");
    assert!(answer.contains("\"finish_reason\":\"stop\""), "{answer}");
    // The strict-string check: the two lines, with their line break.
    let lines = format!(
        "Call probe_lines once. Set value to these two lines, with the line break between them:\n{NONCE}\n{NONCE}"
    );
    let (name, arguments) = call(&server.complete(check(&lines, &["probe_lines"])).await?)?;
    assert_eq!(
        (name.as_str(), arguments),
        ("probe_lines", json!({"value": format!("{NONCE}\n{NONCE}")}))
    );
    // Without native tools the check asks for text.
    assert!(
        !text(&server.complete(check(&echo(), &[])).await?)?
            .trim()
            .is_empty()
    );
    // None of it used a scripted reply: the first task request still gets the first.
    assert_eq!(text(&server.complete(request(0)).await?)?, "scripted-0");
    // The check's tool alone is not enough: another request that offers it goes to the scenario as usual.
    let other = check("Please call probe_echo for me.", &["probe_echo"]);
    assert_eq!(text(&server.complete(other).await?)?, "scripted-1");
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn tk_u_037_prompt_selection_never_refuses_the_check() -> TestResult {
    let server = Binary::start(&scenario("prompt", Some("A task's own first message"))).await?;
    let (name, _) = call(&server.complete(check(&echo(), &["probe_echo"])).await?)?;
    assert_eq!(name, "probe_echo");
    server.stop().await?;
    Ok(())
}
