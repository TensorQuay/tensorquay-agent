//! TK-U-019: a fake endpoint whose constrained decoding loses line breaks in strict tools' string arguments, as a
//! local server was measured to (docs/evals/gate-5.md, "Cause found"). Only a tool the request sent with
//! `strict: true` is affected, in the Chat and Responses shapes, and `Strip` removes the line breaks, nested ones too.
//! Off by default.

use serde_json::{Value, json};
use std::{error::Error, sync::Arc};
use tqa_testkit::{
    FakeClock, FakeModelOptions, FakeModelServer, FakeSleeper, Script, Scripted, StrictLineBreaks,
};

type TestResult = Result<(), Box<dyn Error>>;

fn call(name: &str, value: &str) -> Scripted {
    Scripted::ToolCall {
        name: name.into(),
        arguments_json: json!({"value": value, "path": "a.txt", "edits": [{"old_text": "a\n  b"}]})
            .to_string(),
        split_into: 2,
    }
}

fn script() -> Script {
    Script::new(vec![
        call("edit", "first\nsecond\r\nthird"),
        call("note", "one\ntwo"),
    ])
}

/// The request: `edit` strict, `note` not, in the endpoint's tool shape.
fn request(responses: bool) -> Value {
    let parameters = json!({"type":"object","properties":{"value":{"type":"string"},"path":{"type":"string"}},
                            "required":["value","path"],"additionalProperties":false});
    let tool = |name: &str, strict: bool| {
        if responses {
            json!({"type":"function","name":name,"parameters":parameters,"strict":strict})
        } else {
            json!({"type":"function","function":{"name":name,"parameters":parameters,"strict":strict}})
        }
    };
    let mut body =
        json!({"model":"m","stream":true,"tools":[tool("edit", true), tool("note", false)]});
    if responses {
        body["input"] = json!([]);
    } else {
        body["messages"] = json!([]);
    }
    body
}

/// Each call's (name, arguments) from the stream, in order.
async fn calls(
    mode: Option<StrictLineBreaks>,
    responses: bool,
) -> Result<Vec<(String, Value)>, Box<dyn Error>> {
    let options = FakeModelOptions {
        strict_line_breaks: mode,
        ..FakeModelOptions::default()
    };
    let sleeper = Arc::new(FakeSleeper::new(FakeClock::new()));
    let server = FakeModelServer::start_with(options, vec![script()], sleeper).await?;
    let path = if responses {
        "responses"
    } else {
        "chat/completions"
    };
    let text = reqwest::Client::new()
        .post(format!("{}/{path}", server.base_url()))
        .json(&request(responses))
        .send()
        .await?
        .text()
        .await?;
    server.shutdown().await?;
    let events: Vec<Value> = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect();
    let mut found: Vec<(String, String)> = Vec::new();
    for event in events {
        if responses {
            if event["type"] == "response.output_item.done"
                && event["item"]["type"] == "function_call"
            {
                let item = &event["item"];
                found.push((
                    item["name"].as_str().unwrap_or_default().into(),
                    item["arguments"].as_str().unwrap_or_default().into(),
                ));
            }
            continue;
        }
        for delta in event["choices"][0]["delta"]["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(name) = delta["function"]["name"].as_str() {
                found.push((name.into(), String::new()));
            }
            if let (Some(last), Some(part)) =
                (found.last_mut(), delta["function"]["arguments"].as_str())
            {
                last.1.push_str(part);
            }
        }
    }
    found
        .into_iter()
        .map(|(name, arguments)| Ok((name, serde_json::from_str(&arguments)?)))
        .collect()
}

fn values(calls: &[(String, Value)]) -> Vec<(&str, &str)> {
    calls
        .iter()
        .map(|(name, arguments)| {
            (
                name.as_str(),
                arguments["value"].as_str().unwrap_or_default(),
            )
        })
        .collect()
}

// TEST: TK-U-019
#[tokio::test]
async fn tk_u_019_off_by_default_every_string_keeps_its_line_breaks() -> TestResult {
    for responses in [false, true] {
        let seen = calls(None, responses).await?;
        assert_eq!(
            values(&seen),
            [("edit", "first\nsecond\r\nthird"), ("note", "one\ntwo")],
            "responses={responses}"
        );
    }
    Ok(())
}

// TEST: TK-U-019
#[tokio::test]
async fn tk_u_019_strip_removes_line_breaks_only_from_strict_tools() -> TestResult {
    for responses in [false, true] {
        let seen = calls(Some(StrictLineBreaks::Strip), responses).await?;
        assert_eq!(
            values(&seen),
            [("edit", "firstsecondthird"), ("note", "one\ntwo")],
            "responses={responses}"
        );
        assert_eq!(seen[0].1["path"], "a.txt", "other arguments are kept");
        assert_eq!(
            seen[0].1["edits"][0]["old_text"], "a  b",
            "nested strings too"
        );
        assert_eq!(
            seen[1].1["edits"][0]["old_text"], "a\n  b",
            "the non-strict tool keeps its own"
        );
    }
    Ok(())
}
