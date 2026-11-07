//! A decision engine reached through the gateway relay (ADR-0013 point 6): the runtime posts `decision/v1` to
//! `<relay>/v1/before_step` or `/v1/after_response` with its own token, and the gateway adds the engine's credential. The
//! runtime never holds the engine's credential or address. A timeout, a refusal or a malformed answer becomes the
//! loop's [`EngineFailure`], so the step falls back visibly.
use async_trait::async_trait;
use futures::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};
use tqa_core::{Decision, DecisionEngine, EngineFailure, ModelResponse, PlannedAction, RunState};
use tqa_llm::ToolCall;
use tqa_types::{DecisionExposure, DecisionPoint, decision_v1};
use url::Url;

/// The default time an engine has to answer.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(2);
/// The longest time an engine may be given.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(10);

/// What the runtime knows of an engine route and the step's surroundings.
pub struct RelayEngineConfig {
    /// The engine's route name.
    pub name: String,
    /// The engine's relay base, `<gateway origin>/relay/d/<engine>`.
    pub base: Url,
    /// The runtime's own token for the relay.
    pub token: SecretString,
    /// The time the engine has to answer; zero means the default, and it is at most [`MAX_TIMEOUT`].
    pub timeout: Duration,
    /// How much of the conversation the engine may see.
    pub exposure: DecisionExposure,
    /// The registry's tools: name, description and parameter schema.
    pub tools: Vec<(String, String, Value)>,
    /// The routes' facts, for the request.
    pub routes: Vec<decision_v1::Route>,
    /// The points the engine answers; `None` means both.
    pub points: Option<Vec<DecisionPoint>>,
}

/// The engine behind the relay.
pub struct RelayDecisionEngine {
    name: String,
    base: String,
    token: SecretString,
    timeout: Duration,
    exposure: DecisionExposure,
    client: reqwest::Client,
    tools: BTreeMap<String, (String, Value)>,
    routes: BTreeMap<String, decision_v1::Route>,
    points: Option<Vec<DecisionPoint>>,
}

impl RelayDecisionEngine {
    /// The engine for `config`.
    ///
    /// # Errors
    /// A failure to build the HTTP client.
    pub fn new(config: RelayEngineConfig) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let timeout = if config.timeout.is_zero() {
            DEFAULT_TIMEOUT
        } else {
            config.timeout.min(MAX_TIMEOUT)
        };
        Ok(Self {
            name: config.name,
            base: config.base.as_str().trim_end_matches('/').to_owned(),
            token: config.token,
            timeout,
            exposure: config.exposure,
            client,
            tools: config
                .tools
                .into_iter()
                .map(|(name, description, parameters)| (name, (description, parameters)))
                .collect(),
            routes: config
                .routes
                .into_iter()
                .map(|route| (route.name.clone(), route))
                .collect(),
            points: config.points,
        })
    }

    /// The time the engine has to answer.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// The request for `state` at `point`; for `after_response`, the response's calls as the exposure allows.
    fn request(
        &self,
        point: DecisionPoint,
        state: &RunState,
        response: Option<&ModelResponse>,
    ) -> decision_v1::Request {
        let calls = self.exposure >= DecisionExposure::Calls;
        let response_calls =
            response.map_or_else(Vec::new, |response| self.response_calls(response));
        decision_v1::Request {
            point,
            run_id: state.run_id.clone(),
            step: state.step,
            attempt: state.attempt,
            routes: self.routes(state),
            tools: self.tools(state),
            budget: decision_v1::Budget {
                steps_used: state.budget.steps_used,
                max_steps: state.budget.max_steps,
                output_tokens_used: state.budget.output_tokens_used,
                max_output_tokens: state.budget.max_output_tokens,
            },
            // A call whose name the model made up (not a tool name) is left out: it couldn't run, and the relay refuses
            // a name that could carry text.
            previous_calls: state
                .previous_calls
                .iter()
                .filter(|call| tqa_types::valid_tool_name(&call.name))
                .map(|call| decision_v1::Call {
                    name: call.name.clone(),
                    status: call.status,
                    arguments: calls.then(|| call.arguments.clone()).flatten(),
                })
                .collect(),
            response_calls,
            response_text: response
                .filter(|_| self.exposure >= DecisionExposure::Full)
                .map(|response| response.text.clone()),
            transcript: state
                .transcript
                .as_ref()
                .filter(|_| self.exposure >= DecisionExposure::Full)
                .map(|lines| {
                    lines
                        .iter()
                        .map(|line| decision_v1::TranscriptLine {
                            role: line.role.clone(),
                            text: line.text.clone(),
                        })
                        .collect()
                }),
        }
    }

    /// A response's calls by id and name, with their arguments only under exposure `calls` or `full`; a call with a
    /// made-up name is left out, as its id is.
    fn response_calls(&self, response: &ModelResponse) -> Vec<decision_v1::ResponseCall> {
        let calls = self.exposure >= DecisionExposure::Calls;
        response
            .tool_calls
            .iter()
            .filter(|call| tqa_types::valid_tool_name(&call.name))
            .filter_map(|call| {
                Some(decision_v1::ResponseCall {
                    id: call.id.parse().ok()?,
                    name: call.name.clone(),
                    arguments: calls
                        .then(|| serde_json::from_str(&call.arguments).ok())
                        .flatten(),
                })
            })
            .collect()
    }
    /// The step's allowed routes with the facts known for them; a route without facts goes by name.
    fn routes(&self, state: &RunState) -> Vec<decision_v1::Route> {
        state
            .routes
            .iter()
            .map(|name| {
                self.routes
                    .get(name)
                    .cloned()
                    .unwrap_or(decision_v1::Route {
                        name: name.clone(),
                        context_window: None,
                        max_output: None,
                        native_tools: true,
                    })
            })
            .collect()
    }
    /// The step's tools with their descriptions and parameter schemas.
    fn tools(&self, state: &RunState) -> Vec<decision_v1::Tool> {
        state
            .tools
            .iter()
            .map(|name| {
                let (description, parameters) = self
                    .tools
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| (String::new(), Value::Object(serde_json::Map::new())));
                decision_v1::Tool {
                    name: name.clone(),
                    description,
                    parameters,
                }
            })
            .collect()
    }
    /// Posts `request` to `<relay>/v1/<path>` and reads one decision, within the timeout.
    async fn consult(
        &self,
        path: &str,
        request: &decision_v1::Request,
    ) -> Result<decision_v1::Response, EngineFailure> {
        let exchange = async {
            let response = self
                .client
                .post(format!("{}/v1/{path}", self.base))
                .bearer_auth(self.token.expose_secret())
                .json(request)
                .send()
                .await
                .map_err(|_| EngineFailure::Unreachable)?;
            if !response.status().is_success() {
                return Err(refusal(response).await);
            }
            let body = bounded(response).await?;
            decision_v1::parse_response(&body).map_err(|_| EngineFailure::Invalid)
        };
        tokio::time::timeout(self.timeout, exchange)
            .await
            .map_err(|_| EngineFailure::Timeout)?
    }
}

/// A refused consultation as the loop records it: `invalid` when the gateway says the engine's answer was unusable
/// (`502 decision_engine_invalid`, from a typed-question engine), `budget_spent` when the engine's budget is spent
/// (`409 decision_engine_budget_spent`), otherwise `unreachable`.
async fn refusal(response: reqwest::Response) -> EngineFailure {
    let expected = match response.status() {
        reqwest::StatusCode::BAD_GATEWAY => ("decision_engine_invalid", EngineFailure::Invalid),
        reqwest::StatusCode::CONFLICT => {
            ("decision_engine_budget_spent", EngineFailure::BudgetSpent)
        }
        _ => return EngineFailure::Unreachable,
    };
    let code_matches = bounded(response)
        .await
        .ok()
        .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
        .is_some_and(|body| body["error"]["code"] == expected.0);
    if code_matches {
        expected.1
    } else {
        EngineFailure::Unreachable
    }
}

#[async_trait]
impl DecisionEngine for RelayDecisionEngine {
    fn name(&self) -> Option<&str> {
        Some(&self.name)
    }
    fn answers(&self, point: DecisionPoint) -> bool {
        self.points
            .as_ref()
            .is_none_or(|points| points.contains(&point))
    }
    fn exposure(&self) -> DecisionExposure {
        self.exposure
    }
    async fn before_step(&self, state: &RunState) -> Result<Decision, EngineFailure> {
        let request = self.request(DecisionPoint::BeforeStep, state, None);
        match self.consult("before_step", &request).await? {
            decision_v1::Response::CallModel { route, tools } => Ok(Decision::CallModel {
                route,
                tool_subset: tools,
            }),
            decision_v1::Response::Act { tool, arguments } => Ok(Decision::Act(ToolCall {
                id: String::new(),
                name: tool,
                arguments: arguments.to_string(),
            })),
            decision_v1::Response::Compact {} => Ok(Decision::Compact),
            decision_v1::Response::Stop { reason } => Ok(Decision::Stop(reason)),
            // A selection answers only a response.
            decision_v1::Response::Select { .. } => Err(EngineFailure::Invalid),
        }
    }
    async fn after_response(
        &self,
        state: &RunState,
        response: &ModelResponse,
    ) -> Result<Vec<PlannedAction>, EngineFailure> {
        let request = self.request(DecisionPoint::AfterResponse, state, Some(response));
        match self.consult("after_response", &request).await? {
            decision_v1::Response::Select { call_ids } => Ok(call_ids
                .into_iter()
                .map(|id| PlannedAction {
                    call_id: id.to_string(),
                })
                .collect()),
            _ => Err(EngineFailure::Invalid),
        }
    }
}

/// The answer's body, refused past [`decision_v1::MAX_RESPONSE_BYTES`].
async fn bounded(response: reqwest::Response) -> Result<Vec<u8>, EngineFailure> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| EngineFailure::Unreachable)?;
        if body.len() + chunk.len() > decision_v1::MAX_RESPONSE_BYTES {
            return Err(EngineFailure::Invalid);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
