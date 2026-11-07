//! Shared synthetic requests and explicit test-provider construction.

use serde_json::json;
use std::{error::Error, sync::Arc};
use tqa_llm::{
    CompletionRequest, ContentPart, HttpConfig, Message, ModelProvider, Registry, Role, Route,
    StreamLimits, ToolSpec, provider_for,
};
use tqa_testkit::{FakeClock, FakeSleeper};

pub fn route(base_url: &str) -> Result<Route, Box<dyn Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='test'\nprotocol='openai_chat'\nbase_url='{base_url}'\nmodel='fake-model'\nauth={{kind='none'}}\nstream_usage=true\n"
    ))?;
    registry
        .get("test")
        .cloned()
        .ok_or_else(|| "missing route".into())
}

pub fn message(role: Role, text: &str) -> Message {
    Message {
        role,
        content: vec![ContentPart::Text(text.into())],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}

pub fn request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![
            message(Role::System, "Keep answers brief."),
            message(Role::User, "Inspect the file."),
        ],
        ..CompletionRequest::default()
    }
}

pub fn tool() -> ToolSpec {
    ToolSpec {
        name: "read_file".into(),
        description: "Read text".into(),
        // Strict-valid, like a builtin's schema, so the route's default (auto) sends it strict.
        parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        strict: true,
    }
}

pub fn provider(route: Route, clock: &FakeClock) -> Result<Arc<dyn ModelProvider>, Box<dyn Error>> {
    Ok(provider_for(
        route,
        None,
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock.clone())),
    )?)
}
