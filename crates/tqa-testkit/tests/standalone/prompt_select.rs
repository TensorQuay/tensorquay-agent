//! TK-U-036: prompt selection. With `"select": "prompt"` every reply names a `when` phrase; a request gets the replies
//! whose phrase appears in its conversation's first user message, in order, indexed by its turn (the last repeats), on
//! every wire. So parallel conversations on one stack each follow their own script, whatever the arrival order.
use super::support::*;
use serde_json::{Value, json};
use std::{io::Write, time::Duration};
use tokio::process::Command;

fn reply(when: Option<&str>, text: &str) -> Value {
    let mut reply = json!({"events": [{"type": "text", "text": text}], "usage": {"prompt_tokens": 1, "completion_tokens": 1}});
    if let Some(when) = when {
        reply["when"] = json!(when);
    }
    reply
}

fn scenario() -> String {
    json!({"model": "fixture", "select": "prompt", "responses": [
        reply(Some("Hold the slot"), "held-0"),
        reply(Some("Answer at once"), "answer-0"),
        reply(Some("Hold the slot"), "held-1"),
    ]})
    .to_string()
}

fn chat(first: &str, turns: usize) -> Value {
    let mut body = request(turns);
    body["messages"][0]["content"] = json!(first);
    body
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

#[tokio::test]
async fn tk_u_036_each_conversation_follows_its_own_replies_by_turn() -> TestResult {
    let server = Binary::start(&scenario()).await?;
    for (first, turns, expected) in [
        ("Please Hold the slot for this task.", 0, "held-0"),
        ("Answer at once, please.", 0, "answer-0"),
        ("Please Hold the slot for this task.", 1, "held-1"),
        ("Please Hold the slot for this task.", 2, "held-1"),
        ("Answer at once, please.", 3, "answer-0"),
    ] {
        let (status, wire) = post(&server, "/v1/chat/completions", &chat(first, turns)).await?;
        assert_eq!(status, 200);
        assert!(wire.contains(expected), "{first} turn {turns}: {wire}");
    }
    let (status, _) = post(
        &server,
        "/v1/chat/completions",
        &chat("Nothing scripted", 0),
    )
    .await?;
    assert_eq!(
        status, 422,
        "a conversation no reply names is refused, visibly"
    );
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn tk_u_036_the_first_user_message_is_read_on_every_wire() -> TestResult {
    let server = Binary::start(&scenario()).await?;
    let responses = json!({"model": "fixture", "stream": true, "input": [
        {"type": "message", "role": "system", "content": "Hold the slot is not a user's phrase here"},
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Answer at once."}]}]});
    let (status, wire) = post(&server, "/v1/responses", &responses).await?;
    assert_eq!(status, 200);
    assert!(wire.contains("answer-0"), "{wire}");
    let messages = json!({"model": "fixture", "stream": true, "max_tokens": 64, "messages": [
        {"role": "user", "content": [{"type": "text", "text": "Hold the slot."}]}]});
    let (status, wire) = post(&server, "/v1/messages", &messages).await?;
    assert_eq!(status, 200);
    assert!(wire.contains("held-0"), "{wire}");
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn tk_u_036_when_goes_with_prompt_selection_only() -> TestResult {
    let mut missing: Value = serde_json::from_str(&scenario())?;
    missing["responses"][1]
        .as_object_mut()
        .ok_or("reply")?
        .remove("when");
    let mut stray: Value = serde_json::from_str(&scenario())?;
    stray.as_object_mut().ok_or("document")?.remove("select");
    let mut empty: Value = serde_json::from_str(&scenario())?;
    empty["responses"][0]["when"] = json!("");
    for scenario in [missing, stray, empty] {
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(scenario.to_string().as_bytes())?;
        let result = tokio::time::timeout(
            tqa_testkit::deadline::scaled(Duration::from_secs(5)),
            Command::new(env!("CARGO_BIN_EXE_tqa-fake-model"))
                .arg("--scenario")
                .arg(file.path())
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        assert!(!result.status.success(), "accepted {scenario}");
        assert!(
            String::from_utf8(result.stderr)?.contains("when"),
            "{scenario}"
        );
    }
    Ok(())
}
