//! TK-U-038: a scenario reply can be cut as a model server cuts one at its output limit (`"finish": "length"`), on a
//! text, a reasoning-only and a tool-call reply, whose arguments a `"cut": true` sends only in part; on every wire.
//! TK-U-039: a `"once": true` reply (prompt selection) is served once per conversation, then gives way to the next reply
//! naming the same phrase, so a new message after a cut gets an answer.
//! TK-U-040: `"by_model"` makes a model name fail on request, before the route check and any scripted reply: an HTTP
//! status with the provider's body and Retry-After, a text-only answer, or a tool call with raw arguments; `/v1/models`
//! names those models.
use super::support::*;
use serde_json::{Value, json};
use std::{io::Write, time::Duration};
use tokio::process::Command;

fn usage() -> Value {
    json!({"prompt_tokens": 10, "completion_tokens": 64})
}

fn scenario(reply: &Value) -> String {
    json!({"model": "fixture", "responses": [reply]}).to_string()
}

async fn post(
    server: &Binary,
    path: &str,
    body: &Value,
) -> Result<(u16, String), Box<dyn std::error::Error>> {
    let response = server
        .client
        .post(format!("{}{path}", server.base))
        .bearer_auth(TOKEN)
        .json(body)
        .send()
        .await?;
    Ok((response.status().as_u16(), response.text().await?))
}

fn finish(wire: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(chunks(wire)?
        .iter()
        .filter_map(|chunk| {
            chunk["choices"][0]["finish_reason"]
                .as_str()
                .map(str::to_owned)
        })
        .collect())
}

fn arguments(wire: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(chunks(wire)?
        .iter()
        .flat_map(|chunk| {
            chunk["choices"][0]["delta"]["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|delta| delta["function"]["arguments"].as_str().map(str::to_owned))
        .collect())
}

async fn refused(scenario: &str) -> TestResult {
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(scenario.as_bytes())?;
    let result = tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        Command::new(env!("CARGO_BIN_EXE_tqa-fake-model"))
            .arg("--scenario")
            .arg(file.path())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    assert!(!result.status.success(), "accepted: {scenario}");
    Ok(())
}

#[tokio::test]
async fn tk_u_038_a_cut_text_or_reasoning_reply_ends_at_length_on_every_wire() -> TestResult {
    let text = Binary::start(&scenario(
        &json!({"events": [{"type": "text", "text": "The first half of"}], "usage": usage(), "finish": "length"}),
    ))
    .await?;
    let wire = text.complete(request(0)).await?;
    assert_eq!(finish(&wire)?, ["length"], "{wire}");
    let messages = json!({"model": "fixture", "stream": true, "max_tokens": 64,
        "messages": [{"role": "user", "content": "Write it all."}]});
    let (status, wire) = post(&text, "/v1/messages", &messages).await?;
    assert!(
        status == 200 && wire.contains("\"stop_reason\":\"max_tokens\""),
        "{wire}"
    );
    let responses = json!({"model": "fixture", "stream": true,
        "input": [{"type": "message", "role": "user", "content": "Write it all."}]});
    let (status, wire) = post(&text, "/v1/responses", &responses).await?;
    assert!(
        status == 200
            && wire.contains("\"status\":\"incomplete\"")
            && wire.contains("max_output_tokens"),
        "{wire}"
    );
    text.stop().await?;
    // A thinking model can spend its whole limit on reasoning: no text at all.
    let thinking = Binary::start(&scenario(
        &json!({"events": [{"type": "reasoning", "text": "Let me think about"}], "usage": usage(), "finish": "length"}),
    ))
    .await?;
    let wire = thinking.complete(request(0)).await?;
    assert_eq!(finish(&wire)?, ["length"]);
    assert!(
        !chunks(&wire)?
            .iter()
            .any(|chunk| chunk["choices"][0]["delta"]["content"].is_string()),
        "{wire}"
    );
    thinking.stop().await?;
    Ok(())
}

#[tokio::test]
async fn tk_u_038_a_cut_tool_call_sends_only_part_of_its_arguments() -> TestResult {
    let full = json!({"command": "printf 'a long enough command to be cut in half'"});
    let server = Binary::start(&scenario(&json!({"events": [
        {"type": "tool_call", "name": "shell", "arguments": full, "split_into": 2, "cut": true}],
        "usage": usage(), "finish": "length"})))
    .await?;
    let wire = server.complete(request(0)).await?;
    assert_eq!(finish(&wire)?, ["length"]);
    let sent = arguments(&wire)?;
    let whole = full.to_string();
    assert!(
        !sent.is_empty() && sent.len() < whole.len() && whole.starts_with(&sent),
        "{sent}"
    );
    assert!(
        serde_json::from_str::<Value>(&sent).is_err(),
        "the cut arguments are incomplete JSON"
    );
    server.stop().await?;
    // `cut` goes with a length stop only.
    refused(&scenario(&json!({"events": [
        {"type": "tool_call", "name": "shell", "arguments": full, "split_into": 1, "cut": true}], "usage": usage()})))
    .await?;
    refused(&scenario(
        &json!({"events": [{"type": "text", "text": "x"}], "usage": usage(), "finish": "stop"}),
    ))
    .await
}

fn prompt_reply(when: &str, text: &str, once: bool) -> Value {
    let mut reply =
        json!({"events": [{"type": "text", "text": text}], "usage": usage(), "when": when});
    if once {
        reply["once"] = json!(true);
        reply["finish"] = json!("length");
    }
    reply
}

fn chat(first: &str) -> Value {
    let mut body = request(0);
    body["messages"][0]["content"] = json!(first);
    body
}

#[tokio::test]
async fn tk_u_039_a_once_reply_gives_way_to_the_next_for_the_same_conversation() -> TestResult {
    let server = Binary::start(
        &json!({"model": "fixture", "select": "prompt", "responses": [
            prompt_reply("Cut the text", "The first half of", true),
            prompt_reply("Cut the text", "The whole answer.", false),
            prompt_reply("Another task", "Other.", false),
        ]})
        .to_string(),
    )
    .await?;
    // The cut attempt leaves the history as it was, so the new message's request is at the same turn.
    let first = server.complete(chat("Cut the text, please.")).await?;
    assert_eq!(finish(&first)?, ["length"]);
    let again = server.complete(chat("Cut the text, please.")).await?;
    assert!(
        again.contains("The whole answer.") && finish(&again)? == ["stop"],
        "{again}"
    );
    // Once is per conversation: another conversation with the same phrase is cut once too, and others are untouched.
    assert_eq!(
        finish(&server.complete(chat("Cut the text, now.")).await?)?,
        ["length"]
    );
    assert!(
        server
            .complete(chat("Another task."))
            .await?
            .contains("Other.")
    );
    server.stop().await?;
    // `once` goes with prompt selection only.
    refused(&scenario(
        &json!({"events": [{"type": "text", "text": "x"}], "usage": usage(), "once": true}),
    ))
    .await
}

fn by_model() -> String {
    json!({"model": "fixture", "responses": [{"events": [{"type": "text", "text": "scripted"}], "usage": usage()}],
        "by_model": {
            "fail-401": {"status": 401, "body": "{\"error\":{\"message\":\"Invalid API key\"}}"},
            "fail-429": {"status": 429, "body": "{\"error\":{\"message\":\"Slow down\"}}", "retry_after": "5"},
            "text-only": {"text": "I can't call tools."},
            "bad-tool-call": {"tool_call": {"arguments": "[1, 2]"}},
            "broken-args": {"tool_call": {"arguments": "{not json"}},
            "cut": {"text": "Half of", "finish": "length"}
        }})
    .to_string()
}

fn for_model(model: &str) -> Value {
    let mut body = request(0);
    body["model"] = json!(model);
    body["tools"] = json!([{"type": "function", "function": {"name": "probe_echo", "description": "check",
        "parameters": {"type": "object"}}}]);
    body["messages"][0]["content"] = json!("Call probe_echo with value a1b2c3d4.");
    body
}

#[tokio::test]
async fn tk_u_040_a_model_name_fails_as_asked_before_the_check_and_the_script() -> TestResult {
    let server = Binary::start(&by_model()).await?;
    let path = "/v1/chat/completions";
    let (status, body) = post(&server, path, &for_model("fail-401")).await?;
    assert_eq!(
        (status, body.as_str()),
        (401, "{\"error\":{\"message\":\"Invalid API key\"}}")
    );
    let response = server
        .client
        .post(format!("{}{path}", server.base))
        .bearer_auth(TOKEN)
        .json(&for_model("fail-429"))
        .send()
        .await?;
    assert_eq!(response.status().as_u16(), 429);
    assert_eq!(response.headers()["retry-after"], "5");
    // A text-only answer even though the request is a route check with its tool.
    let (_, wire) = post(&server, path, &for_model("text-only")).await?;
    assert!(
        wire.contains("I can't call tools.") && arguments(&wire)?.is_empty(),
        "{wire}"
    );
    // Raw arguments, named after the request's first tool: JSON that isn't an object, and text that isn't JSON.
    let (_, wire) = post(&server, path, &for_model("bad-tool-call")).await?;
    assert!(wire.contains("probe_echo"), "{wire}");
    assert_eq!(arguments(&wire)?, "[1, 2]");
    let (_, wire) = post(&server, path, &for_model("broken-args")).await?;
    assert_eq!(arguments(&wire)?, "{not json");
    let (_, wire) = post(&server, path, &for_model("cut")).await?;
    assert_eq!(finish(&wire)?, ["length"]);
    // The scenario's own model is untouched, and the list names every model.
    assert!(server.complete(request(0)).await?.contains("scripted"));
    let listed: Value = server
        .client
        .get(format!("{}/v1/models", server.base))
        .bearer_auth(TOKEN)
        .send()
        .await?
        .json()
        .await?;
    let mut names: Vec<&str> = listed["data"]
        .as_array()
        .ok_or("data")?
        .iter()
        .filter_map(|model| model["id"].as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "bad-tool-call",
            "broken-args",
            "cut",
            "fail-401",
            "fail-429",
            "fixture",
            "text-only"
        ]
    );
    server.stop().await?;
    // Exactly one behaviour per model, and a status is an error status.
    for bad in [
        json!({"text": "x", "status": 500}),
        json!({}),
        json!({"status": 200}),
    ] {
        let mut document: Value = serde_json::from_str(&by_model())?;
        document["by_model"]["bad"] = bad;
        refused(&document.to_string()).await?;
    }
    Ok(())
}
