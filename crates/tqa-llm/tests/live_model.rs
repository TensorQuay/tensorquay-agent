//! LLM-I-014 (live): a streaming tool round trip with usage against a real OpenAI-compatible server.
//!
//! Opt-in only: it needs a running model endpoint, so it is ignored by default and fails loudly when forced
//! without one. Real time is used because the subject is a real server; every wait is bounded by the
//! provider's HTTP deadlines. No credential is in the repository: the key is read from the file named by
//! `TQA_LLM_LIVE_KEY_FILE`.
//!
//! ```sh
//! TQA_LLM_LIVE_BASE_URL=<base url> TQA_LLM_LIVE_MODEL=<model> \
//! TQA_LLM_LIVE_KEY_FILE=<key file> [TQA_LLM_LIVE_PROTOCOL=openai_responses] \
//!   cargo nextest run -p tqa-llm --run-ignored only -E 'test(llm_i_014_live)'
//! ```
//! `TQA_LLM_LIVE_REPORT` optionally names a file for the observed call count, reasoning and usage.
#[allow(
    dead_code,
    reason = "This suite uses only some of the shared provider fixtures."
)]
#[path = "fixtures/http.rs"]
mod fixtures;

use secrecy::SecretString;
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    Completion, CompletionRequest, FinishReason, HttpConfig, ModelProvider, Registry, Role,
    StreamLimits, SystemClock, TokioSleeper, collect, provider_for,
};

type TestResult = Result<(), Box<dyn Error>>;

/// The provider for the live endpoint; the route is built exactly as an admin would declare it.
fn live_provider() -> Result<Arc<dyn ModelProvider>, Box<dyn Error>> {
    let base = std::env::var("TQA_LLM_LIVE_BASE_URL").map_err(
        |_| "set TQA_LLM_LIVE_BASE_URL to run the live LLM-I-014 check (see this file's header)",
    )?;
    let model = std::env::var("TQA_LLM_LIVE_MODEL").unwrap_or_else(|_| "qwen3.8-27b".into());
    // `openai_chat` by default; `openai_responses` runs the same round trip through the Responses adapter.
    let protocol = std::env::var("TQA_LLM_LIVE_PROTOCOL").unwrap_or_else(|_| "openai_chat".into());
    let key = match std::env::var("TQA_LLM_LIVE_KEY_FILE") {
        Ok(path) => Some(std::fs::read_to_string(path)?.trim().to_owned()),
        Err(_) => None,
    };
    let auth = if key.is_some() {
        "auth={kind='bearer'}\nkey_ref='secrets/live'\n"
    } else {
        "auth={kind='none'}\n"
    };
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='live'\nprotocol='{protocol}'\nbase_url='{base}'\nmodel='{model}'\n{auth}\
         context_window=65536\nmax_output=4096\nnative_tools=true\nparallel_tools=true\nstream_usage=true\n"
    ))?;
    let route = registry.get("live").ok_or("missing route")?.clone();
    Ok(provider_for(
        route,
        key.map(SecretString::from),
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(SystemClock),
        Arc::new(TokioSleeper),
    )?)
}

async fn complete(
    provider: &Arc<dyn ModelProvider>,
    request: &CompletionRequest,
) -> Result<Completion, Box<dyn Error>> {
    let stream = provider
        .complete(request.clone(), CancellationToken::new())
        .await?;
    Ok(collect(stream, StreamLimits::RECOMMENDED).await?)
}

#[tokio::test]
#[ignore = "needs a real endpoint; set TQA_LLM_LIVE_BASE_URL"]
async fn llm_i_014_live_tool_round_trip_with_usage() -> TestResult {
    let provider = live_provider()?;
    let mut request = CompletionRequest {
        messages: vec![
            // The runner's slots: system prompt, an empty instructions slot, then one more. A strict single-system
            // template refuses a second system message, so each adapter must send them as one (LLM-U-033/037).
            fixtures::message(
                Role::System,
                "You are a coding agent. Use the tools you are given.",
            ),
            fixtures::message(Role::System, ""),
            fixtures::message(Role::System, "Answer in English."),
            fixtures::message(
                Role::User,
                "Read the file README.md with the read_file tool, then summarise it in one sentence.",
            ),
        ],
        tools: vec![fixtures::tool()],
        ..CompletionRequest::default()
    };
    let first = complete(&provider, &request).await?;
    let call = first
        .tool_calls
        .first()
        .ok_or("the model made no tool call")?;
    assert_eq!(call.name, "read_file");
    let arguments: serde_json::Value = serde_json::from_str(&call.arguments)?;
    assert!(arguments["path"].is_string(), "{arguments}");
    assert_eq!(first.finish_reason, Some(FinishReason::ToolCalls));
    assert!(
        first.usage.is_some(),
        "the server reports usage in the stream"
    );
    let mut assistant = fixtures::message(Role::Assistant, &first.text);
    assistant.tool_calls = first.tool_calls.clone();
    request.messages.push(assistant);
    for call in &first.tool_calls {
        let mut result = fixtures::message(
            Role::Tool,
            "# Demo\nA small project that prints a greeting.\n",
        );
        result.tool_call_id = Some(call.id.clone());
        request.messages.push(result);
    }
    let second = complete(&provider, &request).await?;
    assert!(
        !second.text.trim().is_empty(),
        "the answer after the tool result is empty"
    );
    assert_eq!(second.finish_reason, Some(FinishReason::Stop));
    assert!(second.usage.is_some());
    // Observed capabilities that vary by model and route, reported rather than asserted.
    if let Ok(path) = std::env::var("TQA_LLM_LIVE_REPORT") {
        let report = format!(
            "calls={} reasoning_chars={} first_usage={:?} second_usage={:?}\n",
            first.tool_calls.len(),
            first.reasoning.chars().count(),
            first.usage,
            second.usage
        );
        std::fs::write(path, report)?;
    }
    Ok(())
}
