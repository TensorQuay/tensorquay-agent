//! LLM-C-030: the Responses adapter against both Responses provider fakes (the documented API and the observed
//! backend), in CI. The request set is accepted as production sends it; the fakes' refusals are classified and reach
//! the user as the provider's words.
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
const PROFILES: [FakeProfile; 2] = [
    FakeProfile::OpenaiResponses,
    FakeProfile::OpenaiResponsesBackend,
];

fn route(base_url: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='hosted'\nprotocol='openai_responses'\nbase_url='{base_url}'\nmodel='fake-model'\n\
         auth={{kind='none'}}\nmax_output=1000\nnative_tools=true\nparallel_tools=true\n\
         reasoning={{field='reasoning_effort',allowed=['low','high'],default='low'}}\n"
    ))?;
    Ok(registry.get("hosted").ok_or("missing route")?.clone())
}

#[tokio::test]
async fn llm_c_030_the_request_set_is_accepted_as_production_sends_it() -> TestResult {
    for profile in PROFILES {
        let set = request_set();
        let clock = FakeClock::new();
        let server = FakeModelServer::start_as(
            profile,
            answers(set.len()),
            Arc::new(FakeSleeper::new(clock)),
        )
        .await?;
        let route = route(server.base_url())?;
        for (name, request) in &set {
            let chunks = send(&route, "/responses", request)
                .await?
                .map_err(|(status, body)| format!("{profile:?} {name}: refused {status} {body}"))?;
            assert!(
                chunks
                    .iter()
                    .any(|chunk| matches!(chunk, StreamChunk::TextDelta(text) if text == "ok")),
                "{profile:?} {name}: {chunks:?}"
            );
        }
        for recorded in server.requests() {
            assert!(
                recorded.body["instructions"].is_string(),
                "instructions in the top-level field"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn llm_c_030_a_strict_schema_refusal_is_permanent_and_carries_the_providers_words()
-> TestResult {
    for profile in PROFILES {
        let clock = FakeClock::new();
        let server = FakeModelServer::start_as(
            profile,
            answers(1),
            Arc::new(FakeSleeper::new(clock.clone())),
        )
        .await?;
        let provider = provider_for(
            {
                // strict = "on": the default (auto) sends an open schema non-strict, which keeps real runs from being refused.
                let mut route = route(server.base_url())?;
                route.strict = Some("on".into());
                route
            },
            None,
            HttpConfig::new(StreamLimits::RECOMMENDED),
            Arc::new(clock.clone()),
            Arc::new(FakeSleeper::new(clock)),
        )?;
        // A caller that sends an open schema strict (what the runner's gate prevents) is refused by the provider.
        let request = CompletionRequest {
            messages: vec![matrix::message(Role::User, "hi")],
            tools: vec![ToolSpec {
                name: "list_files".into(),
                description: "List".into(),
                parameters: json!({"type":"object","properties":{"path":{"type":"string"}}}),
                strict: true,
            }],
            ..CompletionRequest::default()
        };
        let Err(error) = provider.complete(request, CancellationToken::new()).await else {
            return Err(format!("{profile:?} accepted an open strict schema").into());
        };
        assert!(
            matches!(error, LlmError::Http { status: 400, .. }),
            "{profile:?}: {error:?}"
        );
        assert!(
            !error.retryable(),
            "{profile:?}: a schema refusal is permanent"
        );
        assert_eq!(
            error.provider_message().as_deref(),
            Some(
                "Invalid schema for function 'list_files': In context=(), 'additionalProperties' is required to be supplied and to be false."
            ),
            "{profile:?}"
        );
        assert_eq!(server.requests().len(), 1, "{profile:?}: no retry");
    }
    Ok(())
}

#[tokio::test]
async fn llm_c_030_a_system_input_message_is_refused_as_observed_and_the_adapter_never_sends_one()
-> TestResult {
    let clock = FakeClock::new();
    let server = FakeModelServer::start_as(
        FakeProfile::OpenaiResponsesBackend,
        answers(0),
        Arc::new(FakeSleeper::new(clock)),
    )
    .await?;
    // Today's adapter output never holds one (LLM-U-037) ...
    let route = route(server.base_url())?;
    for (name, request) in request_set() {
        let body: serde_json::Value =
            serde_json::from_slice(&tqa_llm::encode_request(&route, &request)?)?;
        assert!(
            body["input"]
                .as_array()
                .into_iter()
                .flatten()
                .all(|item| item["role"] != "system"),
            "{name}"
        );
    }
    // ... and the shape bug 1 sent is refused as observed, its words reaching the user.
    let old = json!({"model":"fake-model","stream":true,"store":false,"input":[
        {"type":"message","role":"system","content":"You are a coding agent."},
        {"type":"message","role":"user","content":"hello"}]});
    let response = reqwest::Client::new()
        .post(format!("{}/responses", server.base_url()))
        .json(&old)
        .send()
        .await?;
    assert_eq!(response.status().as_u16(), 400);
    let error = LlmError::Http {
        status: 400,
        body_excerpt: response.text().await?,
    };
    assert_eq!(
        error.provider_message().as_deref(),
        Some("System messages are not allowed")
    );
    Ok(())
}
