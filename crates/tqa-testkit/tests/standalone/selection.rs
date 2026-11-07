//! TK-U-010: arrival-order selection and the summary reply, which let a scripted run cross a compaction.
use super::support::*;
use serde_json::{Value, json};

fn scenario(select: Option<&str>, summary: bool) -> String {
    let reply = |text: &str| json!({"events": [{"type": "text", "text": text}], "usage": {"prompt_tokens": 1, "completion_tokens": 1}});
    let mut document = json!({"model": "fixture", "responses": [reply("reply-0"), reply("reply-1"), reply("reply-2")]});
    if let Some(select) = select {
        document["select"] = json!(select);
    }
    if summary {
        document["summary"] = reply("summary-reply");
    }
    document.to_string()
}

/// A request with tools and `turns` earlier model responses in its history.
fn with_tools(turns: usize) -> Value {
    let mut body = request(turns);
    body["tools"] = json!([{"type": "function", "function": {"name": "shell", "parameters": {"type": "object"}}}]);
    body
}

#[tokio::test]
async fn tk_u_010_arrival_order_ignores_the_history_and_repeats_the_last_reply() -> TestResult {
    let server = Binary::start(&scenario(Some("arrival"), false)).await?;
    // The same short history every time, as right after a compaction: arrival order still advances.
    for expected in ["reply-0", "reply-1", "reply-2", "reply-2"] {
        assert!(
            server.complete(with_tools(0)).await?.contains(expected),
            "{expected}"
        );
    }
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn tk_u_010_a_toolless_request_gets_the_summary_without_advancing_the_script() -> TestResult {
    let server = Binary::start(&scenario(Some("arrival"), true)).await?;
    assert!(server.complete(with_tools(5)).await?.contains("reply-0"));
    assert!(server.complete(request(5)).await?.contains("summary-reply"));
    assert!(server.complete(with_tools(1)).await?.contains("reply-1"));
    let logs = server.stop().await?;
    let turns: Vec<Value> = logs
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).map(|record| record["turn"].clone()))
        .collect::<Result<_, _>>()?;
    assert_eq!(
        turns,
        [json!(0), Value::Null, json!(1)],
        "the summary reply uses no script index"
    );
    Ok(())
}

#[tokio::test]
async fn tk_u_010_turn_selection_stays_the_default_and_takes_a_summary_too() -> TestResult {
    let server = Binary::start(&scenario(None, true)).await?;
    assert!(server.complete(with_tools(2)).await?.contains("reply-2"));
    assert!(server.complete(with_tools(0)).await?.contains("reply-0"));
    assert!(server.complete(request(1)).await?.contains("summary-reply"));
    server.stop().await?;
    let plain = Binary::start(&scenario(None, false)).await?;
    assert!(
        plain.complete(request(1)).await?.contains("reply-1"),
        "without a summary, tool-less requests are scripted"
    );
    plain.stop().await?;
    Ok(())
}

#[tokio::test]
async fn tk_u_010_invalid_selection_and_summary_are_refused() -> TestResult {
    let mut unknown: Value = serde_json::from_str(&scenario(Some("arrival"), false))?;
    unknown["select"] = json!("random");
    let mut empty: Value = serde_json::from_str(&scenario(Some("arrival"), true))?;
    empty["summary"]["events"] = json!([]);
    for document in [unknown, empty] {
        assert!(
            Binary::start(&document.to_string()).await.is_err(),
            "{document}"
        );
    }
    Ok(())
}
