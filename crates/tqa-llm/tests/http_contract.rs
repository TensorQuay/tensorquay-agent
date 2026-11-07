//! LLM-C-001/002/013/019/020 and LLM-I-014/021: offline HTTP contracts.

#[path = "fixtures/http.rs"]
mod fixtures;

use secrecy::SecretString;
use serde_json::json;
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    Auth, HttpConfig, Reasoning, Role, StreamLimits, collect, list_models, provider_for,
};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script, Scripted};

#[tokio::test]
async fn llm_c_001_golden_requests_cover_tools_reasoning_and_extra_body()
-> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(
        vec![Script::new(vec![]); 4],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    for case in 0..4 {
        let mut route = fixtures::route(server.base_url())?;
        let mut request = fixtures::request();
        let mut expected = json!({"messages":[{"role":"system","content":"Keep answers brief."},{"role":"user","content":"Inspect the file."}],"model":"fake-model","stream":true,"stream_options":{"include_usage":true}});
        if case >= 1 {
            request.tools.push(fixtures::tool());
            expected["tools"] = json!([{"type":"function","function":{"name":"read_file","description":"Read text","parameters":request.tools[0].parameters,"strict":true}}]);
            expected["parallel_tool_calls"] = json!(false);
        }
        request.reasoning_effort = Some("low".into());
        if case >= 2 {
            route.reasoning = Some(Reasoning {
                field: "reasoning_effort".into(),
                allowed: vec!["low".into()],
                default: "low".into(),
                budget_tokens: None,
            });
            expected["reasoning_effort"] = json!("low");
        }
        if case == 3 {
            route
                .extra_body
                .insert("template_options".into(), json!({"thinking":true}));
            route
                .extra_headers
                .insert("x-feature".into(), "enabled".into());
            expected["template_options"] = json!({"thinking":true});
        }
        let provider = fixtures::provider(route, &clock)?;
        collect(
            provider.complete(request, CancellationToken::new()).await?,
            StreamLimits::RECOMMENDED,
        )
        .await?;
        let records = server.requests();
        assert_eq!(records[case].body, expected);
        assert_eq!(records[case].encoded_body, serde_json::to_vec(&expected)?);
        if case == 3 {
            assert!(records[case].headers.contains_key("x-feature"));
        }
    }
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_i_014_tool_round_trip_with_final_usage() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let usage = tqa_llm::Usage {
        prompt_tokens: 5,
        completion_tokens: 8,
        cached_tokens: Some(2),
        reasoning_tokens: None,
    };
    let scripts = vec![
        Script::new(vec![
            Scripted::Reasoning("plan".into()),
            Scripted::ToolCall {
                name: "read_file".into(),
                arguments_json: "{\"path\":\"README.md\"}".into(),
                split_into: 5,
            },
            Scripted::Usage(usage.clone()),
        ]),
        Script::new(vec![
            Scripted::Text("done".into()),
            Scripted::Usage(usage.clone()),
        ]),
    ];
    let server = FakeModelServer::start(scripts, Arc::new(FakeSleeper::new(clock.clone()))).await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let mut request = fixtures::request();
    request.tools.push(fixtures::tool());
    let first = collect(
        provider
            .complete(request.clone(), CancellationToken::new())
            .await?,
        StreamLimits::RECOMMENDED,
    )
    .await?;
    assert_eq!(first.reasoning, "plan");
    assert_eq!(first.tool_calls[0].arguments, "{\"path\":\"README.md\"}");
    assert_eq!(first.usage, Some(usage.clone()));
    let mut assistant = fixtures::message(Role::Assistant, "");
    assistant.tool_calls = first.tool_calls.clone();
    let mut answer = fixtures::message(Role::Tool, "file contents");
    answer.tool_call_id = Some(first.tool_calls[0].id.clone());
    request.messages.extend([assistant, answer]);
    let second = collect(
        provider.complete(request, CancellationToken::new()).await?,
        StreamLimits::RECOMMENDED,
    )
    .await?;
    assert_eq!(second.text, "done");
    assert_eq!(second.usage, Some(usage));
    let records = server.requests();
    assert_eq!(
        records[1].body["messages"][2]["tool_calls"][0]["function"]["arguments"],
        first.tool_calls[0].arguments
    );
    assert_eq!(
        records[1].body["messages"][3]["tool_call_id"],
        first.tool_calls[0].id
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_c_019_auth_schemes_have_one_redacted_location() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(
        vec![Script::new(vec![]); 4],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let credential = format!("sk-{}", "example".repeat(7));
    for (index, auth) in [
        Auth::None,
        Auth::Bearer,
        Auth::Header {
            name: "x-credential".into(),
        },
        Auth::Query {
            name: "credential".into(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut route = fixtures::route(server.base_url())?;
        route.auth = auth;
        route.key_ref = Some("secrets/test".into());
        let provider = provider_for(
            route,
            Some(SecretString::from(credential.clone())),
            HttpConfig::new(StreamLimits::RECOMMENDED),
            Arc::new(clock.clone()),
            Arc::new(FakeSleeper::new(clock.clone())),
        )?;
        collect(
            provider
                .complete(fixtures::request(), CancellationToken::new())
                .await?,
            StreamLimits::RECOMMENDED,
        )
        .await?;
        let records = server.requests();
        let record = &records[index];
        assert_eq!(record.headers.contains_key("authorization"), index == 1);
        assert_eq!(record.headers.contains_key("x-credential"), index == 2);
        assert_eq!(record.query.contains_key("credential"), index == 3);
        assert!(!format!("{record:?}").contains(&credential));
        assert!(!record.body.to_string().contains(&credential));
    }
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_c_013_relay_only_changes_destination_and_bearer() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(
        vec![Script::new(vec![])],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let mut route = fixtures::route(&format!("{}/", server.base_url()))?;
    route.auth = Auth::Bearer;
    route.key_ref = Some("secrets/relay".into());
    let provider = provider_for(
        route,
        Some(SecretString::from("example-relay")),
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock.clone())),
    )?;
    collect(
        provider
            .complete(fixtures::request(), CancellationToken::new())
            .await?,
        StreamLimits::RECOMMENDED,
    )
    .await?;
    let requests = server.requests();
    assert_eq!(requests[0].path, "/v1/chat/completions");
    assert!(requests[0].headers.contains_key("authorization"));
    assert!(!requests[0].headers.contains_key("x-api-key"));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_i_021_models_present_and_absent() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(vec![], Arc::new(FakeSleeper::new(clock))).await?;
    let route = fixtures::route(server.base_url())?;
    server.set_models(Some(vec!["model-a".into(), "model-b".into()]));
    assert_eq!(list_models(&route, None).await?, ["model-a", "model-b"]);
    server.set_models(None);
    assert_eq!(list_models(&route, None).await?, ["fake-model"]);
    server.shutdown().await?;
    Ok(())
}

#[test]
fn llm_u_010_every_registry_protocol_constructs_its_adapter() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    for protocol in [
        tqa_llm::Protocol::OpenAiChat,
        tqa_llm::Protocol::OpenAiResponses,
        tqa_llm::Protocol::AnthropicMessages,
    ] {
        let mut route = fixtures::route("https://models.example/v1")?;
        route.protocol = protocol;
        route.limits.max_output = Some(1024);
        let provider = provider_for(
            route,
            None,
            HttpConfig::new(StreamLimits::RECOMMENDED),
            Arc::new(clock.clone()),
            Arc::new(FakeSleeper::new(clock.clone())),
        )?;
        assert_eq!(provider.route().protocol, protocol);
    }
    // A protocol this build cannot speak never becomes a route: it is refused at load.
    let unknown =
        "[[routes]]\nname='t'\nprotocol='future_protocol'\nbase_url='https://h/v1'\nmodel='m'\n";
    assert!(tqa_llm::Registry::from_toml(unknown).is_err());
    Ok(())
}
