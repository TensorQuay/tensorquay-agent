//! The typed-question engine kind in the relay (ADR-0013, the adapter amendment): the checked `decision/v1` request
//! becomes one multiple-choice question to the engine's service, "which route for this step", and its answer becomes
//! a `call_model` decision the relay builds itself. The runtime sees only `decision/v1`. A choice below the engine's
//! `min_confidence` takes the conversation's own route. The service's tokens go on the consultation's usage row.
use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Map, Value, json};
use std::{sync::Arc, time::Duration};
use tqa_types::decision_v1;

use super::meter::Meter;
use crate::{GatewayError, app, connectors::Scheme, decision_engines::EngineDocument};

/// The largest question state the relay sends, in bytes (about 4,000 tokens); refused before sending.
const MAX_STATE_BYTES: usize = 16 * 1024;
/// The question, in the service's words for a routing choice.
const INSTRUCTIONS: &str = "Pick the model route for this agent's next step: the cheaper route for a routine step \
    (reading, looking something up, a small change), the default route otherwise.";

/// A question ready to send: the request it asks about, and its body.
pub(super) struct Question<'a> {
    request: &'a decision_v1::Request,
    body: Vec<u8>,
}

/// Sends the engine's service the question (from [`question`]) and answers the runtime with the decision.
///
/// # Errors
/// `502 decision_engine_invalid` for a request the service refused as invalid, or an answer that isn't a usable
/// choice; `502 upstream_unavailable` when the service fails or refuses the key; `504` past the
/// engine's deadline.
pub(super) async fn ask(
    state: &Arc<app::State>,
    document: &EngineDocument,
    question: Question<'_>,
    credential: Option<SecretString>,
    meter: &mut Meter,
) -> Result<Response, GatewayError> {
    let Question { request, body } = question;
    let client = super::decision::client(state, document).await?;
    let mut call = client
        .post(document.url.as_str())
        .header(header::CONTENT_TYPE, "application/json")
        .body(body);
    if let Some(credential) = &credential {
        call = match (document.auth.scheme, document.auth.header.as_deref()) {
            (Scheme::Header, Some(name)) => call.header(name, credential.expose_secret()),
            _ => call.bearer_auth(credential.expose_secret()),
        };
    }
    let deadline = Duration::from_millis(u64::from(document.timeout_ms));
    let exchange = async {
        let upstream = call
            .send()
            .await
            .map_err(|_| GatewayError::UpstreamUnavailable)?;
        match upstream.status() {
            status if status.is_success() => {}
            StatusCode::UNPROCESSABLE_ENTITY => {
                tracing::warn!(engine = %document.name, "the decision engine's service refused the question as invalid");
                return Err(GatewayError::EngineInvalid);
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                tracing::warn!(engine = %document.name, key_ref = crate::secrets::loggable(document.key_ref.as_deref().unwrap_or("none")), "the decision engine's service refused its key");
                return Err(GatewayError::UpstreamUnavailable);
            }
            _ => return Err(GatewayError::UpstreamUnavailable),
        }
        let mut answer = Vec::new();
        let mut stream = upstream.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| GatewayError::UpstreamUnavailable)?;
            if answer.len() + chunk.len() > decision_v1::MAX_RESPONSE_BYTES {
                return Err(GatewayError::EngineInvalid);
            }
            answer.extend_from_slice(&chunk);
        }
        Ok(answer)
    };
    let answer = tokio::select! {
        () = state.cancel.cancelled() => return Err(GatewayError::Cancelled),
        outcome = tokio::time::timeout(deadline, exchange) => outcome.map_err(|_| GatewayError::Timeout)??,
    };
    let answer: Value = serde_json::from_slice(&answer).map_err(|_| GatewayError::EngineInvalid)?;
    meter.observe(usage(&answer));
    let decision = decide(document, request, &answer)?;
    Ok((StatusCode::OK, Json(decision)).into_response())
}

/// The service request: the pinned model, the step's state as the exposure allows it (already checked by the relay),
/// and one choice among the step's routes, each with its hint.
///
/// # Errors
/// `502 decision_engine_invalid` for a state over its bound, before anything is sent or counted.
pub(super) fn question<'a>(
    document: &EngineDocument,
    request: &'a decision_v1::Request,
) -> Result<Question<'a>, GatewayError> {
    let mut step = json!({
        "run": request.run_id,
        "step": request.step,
        "attempt": request.attempt,
        "budget": request.budget,
        "previous_calls": request.previous_calls,
        "tools": request.tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
    });
    if let Some(text) = &request.response_text {
        step["response_text"] = json!(text);
    }
    if let Some(transcript) = &request.transcript {
        step["transcript"] = json!(transcript);
    }
    if serde_json::to_vec(&step)?.len() > MAX_STATE_BYTES {
        return Err(GatewayError::EngineInvalid);
    }
    let criteria: Map<String, Value> = request
        .routes
        .iter()
        .map(|route| {
            let hint = document
                .route_hints
                .get(&route.name)
                .cloned()
                .unwrap_or_else(|| route.name.clone());
            (route.name.clone(), json!(hint))
        })
        .collect();
    let body = serde_json::to_vec(&json!({
        "model": document.model,
        "state": step,
        "questions": {"route": {"type": "choice", "instructions": INSTRUCTIONS, "criteria": criteria}},
    }))?;
    Ok(Question { request, body })
}

/// The decision: `call_model` on the chosen route with every step tool, or on the conversation's own route (the first
/// offered) when the choice is below the engine's threshold. A choice outside the options isn't usable.
fn decide(
    document: &EngineDocument,
    request: &decision_v1::Request,
    answer: &Value,
) -> Result<decision_v1::Response, GatewayError> {
    let route = &answer["answers"]["route"];
    let (Some("choice"), Some(choice), Some(confidence)) = (
        route["type"].as_str(),
        route["choice"].as_str(),
        route["confidence"].as_f64(),
    ) else {
        return Err(GatewayError::EngineInvalid);
    };
    if !request.routes.iter().any(|offered| offered.name == choice) {
        return Err(GatewayError::EngineInvalid);
    }
    let own = request.routes.first().ok_or(GatewayError::EngineInvalid)?;
    let chosen = if confidence < document.min_confidence() {
        own.name.clone()
    } else {
        choice.to_owned()
    };
    Ok(decision_v1::Response::CallModel {
        route: chosen,
        tools: request.tools.iter().map(|tool| tool.name.clone()).collect(),
    })
}

/// The tokens the service reports for the call, for the usage row.
fn usage(answer: &Value) -> Option<tqa_llm::Usage> {
    let reported = &answer["usage"];
    Some(tqa_llm::Usage {
        prompt_tokens: reported["input_tokens"].as_u64()?,
        completion_tokens: reported["output_tokens"].as_u64().unwrap_or(0),
        cached_tokens: None,
        reasoning_tokens: None,
    })
}
