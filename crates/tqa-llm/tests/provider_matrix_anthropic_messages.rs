//! LLM-C-031: the Messages adapter against the Messages provider fake, in CI. The request set is accepted as
//! production sends it; the fake's refusals are classified and reach the user as the provider's words.
#[path = "fixtures/provider_matrix.rs"]
mod matrix;

use matrix::{answers, request_set, send};
use serde_json::json;
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    CompletionRequest, HttpConfig, LlmError, Registry, Role, Route, StreamChunk, StreamLimits,
    ToolSpec, provider_for,
};
use tqa_testkit::{FakeClock, FakeModelServer, FakeProfile, FakeSleeper};

type TestResult = Result<(), Box<dyn Error>>;
const PROFILE: FakeProfile = FakeProfile::AnthropicMessages;

fn route(base_url: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='hosted'\nprotocol='anthropic_messages'\nbase_url='{base_url}'\nmodel='fake-model'\n\
         auth={{kind='none'}}\nmax_output=16000\nnative_tools=true\nparallel_tools=true\n\
         reasoning={{field='thinking',allowed=['low','high'],default='low',budget_tokens={{low=2048,high=8192}}}}\n"
    ))?;
    Ok(registry.get("hosted").ok_or("missing route")?.clone())
}

#[tokio::test]
async fn llm_c_031_the_request_set_is_accepted_as_production_sends_it() -> TestResult {
    let set = request_set();
    let clock = FakeClock::new();
    let server = FakeModelServer::start_as(
        PROFILE,
        answers(set.len()),
        Arc::new(FakeSleeper::new(clock)),
    )
    .await?;
    let route = route(server.base_url())?;
    for (name, request) in &set {
        let chunks = send(&route, "/messages", request)
            .await?
            .map_err(|(status, body)| format!("{name}: refused {status} {body}"))?;
        assert!(
            chunks
                .iter()
                .any(|chunk| matches!(chunk, StreamChunk::TextDelta(text) if text == "ok")),
            "{name}: {chunks:?}"
        );
    }
    for recorded in server.requests() {
        assert!(
            recorded.body["system"].is_array(),
            "system as top-level blocks"
        );
    }
    Ok(())
}

#[tokio::test]
async fn llm_c_031_a_refusal_is_permanent_and_carries_the_providers_words() -> TestResult {
    let clock = FakeClock::new();
    let server = FakeModelServer::start_as(
        PROFILE,
        answers(1),
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let provider = provider_for(
        route(server.base_url())?,
        None,
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock)),
    )?;
    // A tool whose schema isn't an object schema, which the provider refuses.
    let request = CompletionRequest {
        messages: vec![matrix::message(Role::User, "hi")],
        tools: vec![ToolSpec {
            name: "echo".into(),
            description: "Echo".into(),
            parameters: json!({"type":"string"}),
            strict: false,
        }],
        ..CompletionRequest::default()
    };
    let Err(error) = provider.complete(request, CancellationToken::new()).await else {
        return Err("the fake accepted a non-object input schema".into());
    };
    assert!(
        matches!(error, LlmError::Http { status: 400, .. }),
        "{error:?}"
    );
    assert!(!error.retryable(), "a schema refusal is permanent");
    assert_eq!(
        error.provider_message().as_deref(),
        Some("tools.0.custom.input_schema.type: Input should be 'object'")
    );
    assert_eq!(server.requests().len(), 1, "no retry");
    Ok(())
}

#[tokio::test]
async fn llm_c_031_the_adapter_never_sends_a_system_role_and_the_provider_refuses_one() -> TestResult
{
    let clock = FakeClock::new();
    let server =
        FakeModelServer::start_as(PROFILE, answers(0), Arc::new(FakeSleeper::new(clock))).await?;
    let route = route(server.base_url())?;
    for (name, request) in request_set() {
        let body: serde_json::Value =
            serde_json::from_slice(&tqa_llm::encode_request(&route, &request)?)?;
        assert!(
            body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .all(|message| message["role"] != "system"),
            "{name}"
        );
        assert_eq!(
            body["messages"][0]["role"], "user",
            "{name}: the first turn is the user's"
        );
    }
    let old = json!({"model":"fake-model","stream":true,"max_tokens":100,"messages":[
        {"role":"system","content":"s"},{"role":"user","content":"hi"}]});
    let response = reqwest::Client::new()
        .post(format!("{}/messages", server.base_url()))
        .json(&old)
        .send()
        .await?;
    assert_eq!(response.status().as_u16(), 400);
    let error = LlmError::Http {
        status: 400,
        body_excerpt: response.text().await?,
    };
    assert!(
        error
            .provider_message()
            .is_some_and(|message| message.starts_with("messages: Unexpected role \"system\"")),
        "{error:?}"
    );
    Ok(())
}
