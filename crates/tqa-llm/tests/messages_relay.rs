//! LLM-U-030: relay shaping of Messages requests and the stateful usage reader for every protocol.
#[allow(dead_code, reason = "This suite uses only the fixture reader.")]
#[path = "fixtures/sse_server.rs"]
mod sse_server;

use serde_json::{Value, json};
use sse_server::fixture_in;
use std::error::Error;
use tqa_llm::{
    LlmError, Protocol, Registry, Route, Usage,
    relay_shaping::{UsageReader, read_usage, shape_request},
};

type TestResult = Result<(), Box<dyn Error>>;

fn route(extra: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='t'\nprotocol='anthropic_messages'\nbase_url='https://models.example/v1'\n\
         model='pinned-model'\nmax_output=16000\n{extra}"
    ))?;
    Ok(registry.get("t").ok_or("missing route")?.clone())
}
const THINKING: &str = "reasoning={field='thinking',allowed=['low','high'],default='low',\
    budget_tokens={low=2048,high=8192}}\nextra_body={service_tier='auto',metadata_tag={team='a'}}\n";

/// A runtime request with replayed state and cache markers the relay must not touch.
fn request(thinking: &Value) -> Value {
    json!({
        "model":"untrusted","max_tokens":16000,"stream":false,"thinking":thinking,
        "system":[{"type":"text","text":"Rules.","cache_control":{"type":"ephemeral"}}],
        "messages":[{"role":"user","content":[{"type":"text","text":"a"}]},
            {"role":"assistant","content":[{"type":"thinking","thinking":"Plan.","signature":"s1"},
                {"type":"tool_use","id":"c1","name":"read_file","input":{}}]}],
        "metadata_tag":{"team":"b","keep":true}
    })
}

#[test]
fn llm_u_030_relay_pins_the_model_and_keeps_opaque_content() -> TestResult {
    let route = route(THINKING)?;
    let mut body = request(&json!({"type":"enabled","budget_tokens":8192}));
    let original = body.clone();
    shape_request(&route, &mut body)?;
    assert_eq!(body["model"], "pinned-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["thinking"], original["thinking"]);
    assert_eq!(body["messages"], original["messages"]);
    assert_eq!(body["system"], original["system"]);
    assert_eq!(body["service_tier"], "auto");
    assert_eq!(body["metadata_tag"], json!({"team":"a","keep":true}));
    let mut lost = request(&Value::Null);
    lost.as_object_mut().ok_or("object")?.remove("thinking");
    shape_request(&route, &mut lost)?;
    assert!(
        lost.get("thinking").is_none(),
        "a request without thinking stays without it"
    );
    Ok(())
}

#[test]
fn llm_u_030_relay_refuses_thinking_the_route_does_not_map() -> TestResult {
    let thinking = route(THINKING)?;
    let plain = route("")?;
    for (route, value) in [
        (&thinking, json!({"type":"enabled","budget_tokens":4096})),
        (&thinking, json!({"type":"enabled"})),
        (&thinking, json!({"type":"future"})),
        (&thinking, json!("enabled")),
        (&plain, json!({"type":"enabled","budget_tokens":2048})),
    ] {
        let mut body = request(&value);
        let error = shape_request(route, &mut body).err().ok_or("accepted")?;
        assert!(
            matches!(&error, LlmError::Protocol(text) if text.contains("thinking")),
            "{value}: {error}"
        );
    }
    let mut disabled = request(&json!({"type":"disabled"}));
    shape_request(&plain, &mut disabled)?;
    assert!(shape_request(&thinking, &mut json!([])).is_err());
    Ok(())
}

fn events(name: &str) -> Result<Vec<String>, Box<dyn Error>> {
    Ok(fixture_in("messages", name)?
        .split("\n\n")
        .filter_map(|event| event.lines().find_map(|line| line.strip_prefix("data: ")))
        .map(str::to_owned)
        .collect())
}

#[test]
fn llm_u_030_the_reader_combines_usage_split_across_messages_events() -> TestResult {
    let mut reader = UsageReader::new(Protocol::AnthropicMessages);
    assert_eq!(reader.usage(), None);
    let events = events("thinking_text.sse")?;
    reader.observe(&events[0]);
    let started = Usage {
        prompt_tokens: 28,
        completion_tokens: 1,
        cached_tokens: Some(5),
        reasoning_tokens: None,
    };
    assert_eq!(reader.usage(), Some(started.clone()));
    for data in &events[1..] {
        reader.observe(data);
        reader.observe("not json");
    }
    assert_eq!(
        reader.usage(),
        Some(Usage {
            completion_tokens: 15,
            ..started
        })
    );
    Ok(())
}

#[test]
fn llm_u_030_chat_and_responses_readers_keep_the_latest_usage() -> TestResult {
    let chat = [
        r#"{"choices":[{"index":0,"delta":{"content":"a"}}]}"#,
        r#"{"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":1}}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"b"}}]}"#,
        r#"{"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4}}"#,
    ];
    let responses: Vec<String> = fixture_in("responses", "text.sse")?
        .split("\n\n")
        .filter_map(|event| event.lines().find_map(|line| line.strip_prefix("data: ")))
        .map(str::to_owned)
        .collect();
    for (protocol, data) in [
        (
            Protocol::OpenAiChat,
            chat.iter().map(|d| (*d).to_owned()).collect::<Vec<_>>(),
        ),
        (Protocol::OpenAiResponses, responses),
    ] {
        let mut reader = UsageReader::new(protocol);
        let mut latest = None;
        for event in &data {
            reader.observe(event);
            latest = read_usage(protocol, event).or(latest);
            assert_eq!(reader.usage(), latest, "{protocol:?}: {event}");
        }
        assert!(latest.is_some(), "{protocol:?}");
    }
    Ok(())
}
