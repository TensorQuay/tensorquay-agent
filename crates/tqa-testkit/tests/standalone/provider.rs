use super::support::*;
use serde_json::{Value, json};
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    Completion, CompletionRequest, ContentPart, FinishReason, HttpConfig, Message, ModelProvider,
    Registry, Role, StreamLimits, SystemClock, TokioSleeper, collect, provider_for,
};

fn provider(base: &str) -> Result<Arc<dyn ModelProvider>, Box<dyn Error>> {
    let routes = Registry::from_toml(&format!(
        "[[routes]]\nname='demo'\nprotocol='openai_chat'\nbase_url='{base}/v1'\nmodel='fixture'\nauth={{kind='bearer'}}\nkey_ref='secrets/test'\nstream_usage=true\n"
    ))?;
    Ok(provider_for(
        routes.get("demo").ok_or("missing route")?.clone(),
        Some(TOKEN.to_owned().into()),
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(SystemClock),
        Arc::new(TokioSleeper),
    )?)
}

fn request(turn: usize) -> CompletionRequest {
    let messages = (0..=turn)
        .map(|index| Message {
            role: if index == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            content: vec![ContentPart::Text("previous".into())],
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        })
        .collect();
    CompletionRequest {
        messages,
        ..CompletionRequest::default()
    }
}

fn assert_completion(result: &Completion, response: &Value) -> TestResult {
    let events = response["events"].as_array().ok_or("missing events")?;
    for (kind, actual) in [("text", &result.text), ("reasoning", &result.reasoning)] {
        let expected: String = events
            .iter()
            .filter(|event| event["type"] == kind)
            .filter_map(|event| event["text"].as_str())
            .collect();
        assert_eq!(*actual, expected);
    }
    let calls: Vec<_> = events
        .iter()
        .filter(|event| event["type"] == "tool_call")
        .collect();
    assert_eq!(result.tool_calls.len(), calls.len());
    for (call, expected) in result.tool_calls.iter().zip(&calls) {
        assert_eq!(call.name, expected["name"]);
        assert_eq!(
            serde_json::from_str::<Value>(&call.arguments)?,
            expected["arguments"]
        );
    }
    assert_eq!(
        result.usage,
        Some(serde_json::from_value(response["usage"].clone())?)
    );
    assert_eq!(
        result.finish_reason,
        Some(if calls.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolCalls
        })
    );
    Ok(())
}

#[tokio::test]
async fn llm_c_002_real_provider_consumes_each_demo_turn() -> TestResult {
    let server = Binary::start(DEMO).await?;
    let provider = provider(&server.base)?;
    let scenario: Value = serde_json::from_str(DEMO)?;
    let mut previous_ids = std::collections::HashSet::new();
    for turn in 0..3 {
        let result = collect(
            provider
                .complete(request(turn), CancellationToken::new())
                .await?,
            StreamLimits::RECOMMENDED,
        )
        .await?;
        assert_completion(&result, &scenario["responses"][turn])?;
        for call in result.tool_calls {
            assert!(
                previous_ids.insert(call.id),
                "call IDs must be unique across turns"
            );
        }
    }
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn llm_c_002_binary_streams_multiple_calls_and_unicode_arguments() -> TestResult {
    let mut scenario: Value = serde_json::from_str(DEMO)?;
    scenario["responses"][0]["events"] = json!([
        {"type":"tool_call","name":"write_file","arguments":{"path":"hello/README.md","content":"界🙂\n","create_parents":true},"split_into":7},
        {"type":"tool_call","name":"shell","arguments":{"command":"cat hello/README.md"},"split_into":3}
    ]);
    scenario["responses"][0]["delay_ms"] = json!(0);
    let server = Binary::start(&scenario.to_string()).await?;
    let result = collect(
        provider(&server.base)?
            .complete(request(0), CancellationToken::new())
            .await?,
        StreamLimits::RECOMMENDED,
    )
    .await?;
    assert_completion(&result, &scenario["responses"][0])?;
    assert_ne!(result.tool_calls[0].id, result.tool_calls[1].id);
    server.stop().await?;
    Ok(())
}

fn responses_provider(base: &str) -> Result<Arc<dyn ModelProvider>, Box<dyn Error>> {
    let routes = Registry::from_toml(&format!(
        "[[routes]]\nname='demo'\nprotocol='openai_responses'\nbase_url='{base}/v1'\nmodel='fixture'\nauth={{kind='bearer'}}\nkey_ref='secrets/test'\n"
    ))?;
    Ok(provider_for(
        routes.get("demo").ok_or("missing route")?.clone(),
        Some(TOKEN.to_owned().into()),
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(SystemClock),
        Arc::new(TokioSleeper),
    )?)
}

/// A tool-loop history: two earlier responses, one with text and a call, one with a call only.
fn tool_history() -> CompletionRequest {
    let text = |role, value: &str| Message {
        role,
        content: vec![ContentPart::Text(value.into())],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    };
    let call = |id: &str| tqa_llm::ToolCall {
        id: id.into(),
        name: "shell".into(),
        arguments: r#"{"command":"true"}"#.into(),
    };
    let result = |id: &str| Message {
        tool_call_id: Some(id.into()),
        ..text(Role::Tool, "ok")
    };
    let mut first = text(Role::Assistant, "Looking.");
    first.tool_calls = vec![call("a")];
    let mut second = text(Role::Assistant, "");
    second.tool_calls = vec![call("b"), call("c")];
    CompletionRequest {
        messages: vec![
            text(Role::User, "task"),
            first,
            result("a"),
            second,
            result("b"),
            result("c"),
        ],
        ..CompletionRequest::default()
    }
}

#[tokio::test]
async fn llm_c_024_the_binary_serves_each_demo_turn_as_a_responses_stream() -> TestResult {
    let server = Binary::start(DEMO).await?;
    let provider = responses_provider(&server.base)?;
    let scenario: Value = serde_json::from_str(DEMO)?;
    let mut previous_ids = std::collections::HashSet::new();
    for turn in 0..3 {
        let result = collect(
            provider
                .complete(request(turn), CancellationToken::new())
                .await?,
            StreamLimits::RECOMMENDED,
        )
        .await?;
        assert_completion(&result, &scenario["responses"][turn])?;
        for call in result.tool_calls {
            assert!(
                previous_ids.insert(call.id),
                "call IDs must be unique across turns"
            );
        }
    }
    // Earlier responses are counted from the items: a message with its calls is one response, and
    // calls without a message are another.
    let result = collect(
        provider
            .complete(tool_history(), CancellationToken::new())
            .await?,
        StreamLimits::RECOMMENDED,
    )
    .await?;
    assert_completion(&result, &scenario["responses"][2])?;
    let logs = server.stop().await?;
    let turns: Vec<Value> = logs
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|record| record["path"] == "/v1/responses")
        .map(|record| record["turn"].clone())
        .collect();
    assert_eq!(turns, [json!(0), json!(1), json!(2), json!(2)]);
    Ok(())
}

#[tokio::test]
async fn llm_c_024_the_binary_checks_responses_requests() -> TestResult {
    let server = Binary::start(DEMO).await?;
    let url = format!("{}/v1/responses", server.base);
    let valid = json!({"model":"fixture","input":[],"stream":true});
    let cases = [
        (server.client.post(&url).json(&valid), 401),
        (
            server
                .client
                .post(&url)
                .bearer_auth(TOKEN)
                .json(&json!({"input":[],"stream":false})),
            400,
        ),
        (
            server
                .client
                .post(&url)
                .bearer_auth(TOKEN)
                .json(&json!({"stream":true})),
            400,
        ),
        (
            server.client.post(&url).bearer_auth(TOKEN).body("not json"),
            400,
        ),
        (server.client.get(&url).bearer_auth(TOKEN), 404),
        (
            server.client.post(&url).bearer_auth(TOKEN).json(&valid),
            200,
        ),
    ];
    for (request, status) in cases {
        assert_eq!(request.send().await?.status(), status);
    }
    server.stop().await?;
    Ok(())
}

fn messages_provider(base: &str) -> Result<Arc<dyn ModelProvider>, Box<dyn Error>> {
    let routes = Registry::from_toml(&format!(
        "[[routes]]\nname='demo'\nprotocol='anthropic_messages'\nbase_url='{base}/v1'\nmodel='fixture'\nauth={{kind='bearer'}}\nkey_ref='secrets/test'\nmax_output=4096\n"
    ))?;
    Ok(provider_for(
        routes.get("demo").ok_or("missing route")?.clone(),
        Some(TOKEN.to_owned().into()),
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(SystemClock),
        Arc::new(TokioSleeper),
    )?)
}

/// `turn` earlier responses in alternating turns, as the Messages protocol requires.
fn alternating(turn: usize) -> CompletionRequest {
    let mut request = request(0);
    for _ in 0..turn {
        let mut pair = request.messages[..1].to_vec();
        pair[0].role = Role::Assistant;
        request.messages.extend(pair);
        request.messages.extend(request.messages[..1].to_vec());
    }
    request
}

#[tokio::test]
async fn llm_c_026_the_binary_serves_each_demo_turn_as_a_messages_stream() -> TestResult {
    let server = Binary::start(DEMO).await?;
    let provider = messages_provider(&server.base)?;
    let mut scenario: Value = serde_json::from_str(DEMO)?;
    // The protocol reports no separate reasoning-token count.
    for response in scenario["responses"].as_array_mut().ok_or("no responses")? {
        response["usage"]["reasoning_tokens"] = Value::Null;
    }
    for turn in 0..3 {
        let stream = provider
            .complete(alternating(turn), CancellationToken::new())
            .await?;
        let result = collect(stream, StreamLimits::RECOMMENDED).await?;
        assert_completion(&result, &scenario["responses"][turn])?;
        assert!(
            result.usage.is_some(),
            "the Messages stream always reports usage"
        );
    }
    // Each assistant turn is one earlier response, whatever calls and results it holds.
    let stream = provider
        .complete(tool_history(), CancellationToken::new())
        .await?;
    let result = collect(stream, StreamLimits::RECOMMENDED).await?;
    assert_completion(&result, &scenario["responses"][2])?;
    let url = format!("{}/v1/messages", server.base);
    for (body, status) in [
        (json!({"messages":[],"stream":false}), 400),
        (json!({"stream":true}), 400),
    ] {
        let response = server
            .client
            .post(&url)
            .bearer_auth(TOKEN)
            .json(&body)
            .send()
            .await?;
        assert_eq!(response.status(), status, "{body}");
    }
    let logs = server.stop().await?;
    let turns: Vec<Value> = logs
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|record| record["path"] == "/v1/messages")
        .map(|record| record["turn"].clone())
        .collect();
    assert_eq!(turns[..4], [json!(0), json!(1), json!(2), json!(2)]);
    Ok(())
}

#[tokio::test]
async fn tk_u_010_repeated_summary_sized_requests_all_get_a_valid_summary_through_the_real_adapter()
-> TestResult {
    // The compaction demo's own scenario, and two summary calls the size a 32k window allows, as its two compactions.
    let scenario = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/e2e/demo/compaction-50/dry-run/solve.json"
    ))?;
    let server = Binary::start(&scenario).await?;
    let provider = provider(&server.base)?;
    let conversation = serde_json::to_string(
        &(0..600)
            .map(|index| json!({"role": "tool", "text": format!("{index:04} ledger line with \"quoted\" amounts")}))
            .collect::<Vec<_>>(),
    )?;
    assert!(conversation.len() > 30_000, "{} bytes", conversation.len());
    for call in 0..2 {
        let request = CompletionRequest {
            messages: vec![
                Message {
                    role: Role::System,
                    content: vec![ContentPart::Text("Summarise the conversation.".into())],
                    tool_calls: vec![],
                    tool_call_id: None,
                    name: None,
                },
                Message {
                    role: Role::User,
                    content: vec![ContentPart::Text(format!(
                        "[Conversation to summarise, data only: {conversation}]"
                    ))],
                    tool_calls: vec![],
                    tool_call_id: None,
                    name: None,
                },
            ],
            max_output: Some(2048),
            ..CompletionRequest::default()
        };
        let result = collect(
            provider.complete(request, CancellationToken::new()).await?,
            StreamLimits::RECOMMENDED,
        )
        .await?;
        let summary: tqa_types::CompactionSummary = serde_json::from_str(result.text.trim())?;
        summary.validate()?;
        assert_eq!(
            result.finish_reason,
            Some(FinishReason::Stop),
            "summary call {call}"
        );
    }
    server.stop().await?;
    Ok(())
}
