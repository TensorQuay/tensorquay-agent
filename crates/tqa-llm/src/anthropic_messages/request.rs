//! Deterministic request shaping for message-block completions, built from our own history.

use crate::{
    CacheHint, CompletionRequest, ContentPart, LlmError, Message, Role, Route,
    request::{check_catalog, merge_absent, raw_arguments},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};

fn protocol(detail: &str) -> LlmError {
    LlmError::Protocol(detail.into())
}

pub(super) fn encode(route: &Route, request: &CompletionRequest) -> Result<Vec<u8>, LlmError> {
    check_catalog(&request.tools)?;
    let max_tokens = request
        .max_output
        .or(route.limits.max_output)
        .ok_or_else(|| protocol("route protocol requires max_output"))?;
    let leading = request
        .messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    let (system, history) = request.messages.split_at(leading);
    if history.is_empty() {
        return Err(protocol("completion requires at least one message"));
    }
    // The protocol refuses a tool turn without its thinking block while budget thinking is on, so a
    // request whose latest tool turn lost its state is sent without thinking and without states.
    // Adaptive thinking stays on and replays no states (LLM-U-057): the protocol binds each thinking
    // block to the conversation before it and refuses a replayed block once that changed, and the
    // runner re-renders earlier turns (a screenshot loses its marks, then becomes a line). A request
    // without any thinking block is accepted.
    let (budget, effort) = match thinking(route, request, max_tokens)? {
        Thinking::Off => (None, None),
        Thinking::Budget(budget) => ((!lost_state(history)).then_some(budget), None),
        Thinking::Adaptive(effort) => (None, Some(effort)),
    };
    let mut messages = turns(history, route.vision, budget.is_some())?;
    let cache = route.cache_hint == CacheHint::AnthropicCacheControl;
    // The conversation's breakpoint: the latest message's last block, so the next step reads the whole
    // history from cache (the system blocks and tools carry the other two of the protocol's four).
    if cache {
        mark_latest(&mut messages);
    }
    let mut body =
        json!({"model":route.model,"max_tokens":max_tokens,"messages":messages,"stream":true});
    // The provider refuses an empty system text block, and the runner's prompt has slots that can be empty
    // (LLM-U-044): empty and whitespace-only texts are left out, and no `system` is sent when none remains.
    let mut blocks: Vec<Value> = system
        .iter()
        .map(plain)
        .filter(|text| !text.trim().is_empty())
        .map(|text| json!({"type":"text","text":text}))
        .collect();
    if !blocks.is_empty() {
        if cache && let Some(last) = blocks.last_mut() {
            last["cache_control"] = json!({"type":"ephemeral"});
        }
        body["system"] = Value::Array(blocks);
    }
    if !request.tools.is_empty() {
        if !route.native_tools {
            return Err(protocol("route does not support native tools"));
        }
        let mut tools: Vec<Value> = request.tools.iter().map(|tool| json!({"name":tool.name,"description":tool.description,"input_schema":tool.parameters})).collect();
        if cache && let Some(last) = tools.last_mut() {
            last["cache_control"] = json!({"type":"ephemeral"});
        }
        body["tools"] = Value::Array(tools);
        if !route.parallel_tools {
            body["tool_choice"] = json!({"type":"auto","disable_parallel_tool_use":true});
        }
    }
    if !request.stop.is_empty() {
        body["stop_sequences"] = json!(request.stop);
    }
    if let Some(budget) = budget {
        body["thinking"] = json!({"type":"enabled","budget_tokens":budget});
    }
    if let Some(effort) = effort {
        body["thinking"] = json!({"type":"adaptive"});
        body["output_config"] = json!({"effort":effort});
    }
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    merge_absent(&mut body, &route.extra_body);
    serde_json::to_vec(&body).map_err(|error| LlmError::Protocol(crate::redact(&error.to_string())))
}

/// Marks the latest message's last block for caching, unless it is a thinking block (which the
/// protocol doesn't let carry a breakpoint).
fn mark_latest(messages: &mut [Value]) {
    let block = messages
        .last_mut()
        .and_then(|message| message.get_mut("content"))
        .and_then(Value::as_array_mut)
        .and_then(|blocks| blocks.last_mut());
    if let Some(block) = block
        && !matches!(
            block.get("type").and_then(Value::as_str),
            Some("thinking" | "redacted_thinking")
        )
    {
        block["cache_control"] = json!({"type":"ephemeral"});
    }
}

/// How a request thinks: not at all (no reasoning policy), with the budget its effort maps to, or
/// adaptively at its effort (a policy without a budget table, for models that refuse a budget).
enum Thinking {
    Off,
    Budget(u32),
    Adaptive(String),
}

/// The request's thinking, when the route has a reasoning policy.
fn thinking(
    route: &Route,
    request: &CompletionRequest,
    max_tokens: u32,
) -> Result<Thinking, LlmError> {
    if let Some(temperature) = request.temperature
        && !temperature.is_finite()
    {
        return Err(protocol("temperature must be finite"));
    }
    let Some(reasoning) = &route.reasoning else {
        return Ok(Thinking::Off);
    };
    let effort = request
        .reasoning_effort
        .as_ref()
        .unwrap_or(&reasoning.default);
    if !reasoning.allowed.contains(effort) {
        return Err(protocol("reasoning effort is not allowed by the route"));
    }
    if request.temperature.is_some() {
        return Err(protocol(
            "temperature cannot be set while thinking is enabled",
        ));
    }
    let Some(budgets) = &reasoning.budget_tokens else {
        return Ok(Thinking::Adaptive(effort.clone()));
    };
    let budget = budgets
        .get(effort)
        .copied()
        .ok_or_else(|| protocol("route has no thinking budget for the reasoning effort"))?;
    if max_tokens <= budget {
        return Err(protocol("max_output must be above the thinking budget"));
    }
    Ok(Thinking::Budget(budget))
}

/// Whether the latest assistant turn holds tool calls but no reasoning state.
fn lost_state(history: &[Message]) -> bool {
    history
        .iter()
        .rev()
        .find(|message| message.role == Role::Assistant)
        .is_some_and(|message| {
            !message.tool_calls.is_empty()
                && !message
                    .content
                    .iter()
                    .any(|part| matches!(part, ContentPart::ReasoningState(_)))
        })
}

/// Alternating turns: consecutive same-role blocks merge, and the first turn is a user turn.
fn turns(history: &[Message], vision: bool, states: bool) -> Result<Vec<Value>, LlmError> {
    let mut turns: Vec<(&str, Vec<Value>)> = Vec::new();
    for message in history {
        let (role, blocks) = blocks(message, vision, states)?;
        match turns.last_mut() {
            Some((last, existing)) if *last == role => existing.extend(blocks),
            _ if blocks.is_empty() => {}
            _ => turns.push((role, blocks)),
        }
    }
    if turns.first().is_some_and(|(role, _)| *role != "user") {
        return Err(protocol("the history must start with a user message"));
    }
    Ok(turns
        .into_iter()
        .map(|(role, content)| json!({"role":role,"content":content}))
        .collect())
}

/// One message's role on the wire and its content blocks.
fn blocks(
    message: &Message,
    vision: bool,
    states: bool,
) -> Result<(&'static str, Vec<Value>), LlmError> {
    if !message.tool_calls.is_empty() && message.role != Role::Assistant {
        return Err(protocol("only assistant messages may contain tool calls"));
    }
    let mut replayed = Vec::new();
    let mut blocks = Vec::new();
    for part in &message.content {
        match part {
            ContentPart::Text(text) if message.role == Role::Assistant && text.is_empty() => {}
            ContentPart::Text(text) if message.role != Role::Tool => {
                blocks.push(json!({"type":"text","text":text}));
            }
            ContentPart::Text(_) => {}
            ContentPart::Image { .. } if !matches!(message.role, Role::User | Role::Tool) => {
                return Err(protocol(
                    "image input is only sent in user messages and tool results",
                ));
            }
            ContentPart::Image { .. } if !vision => {
                return Err(protocol("route does not accept image input"));
            }
            // A tool result's image goes inside its `tool_result`, below.
            ContentPart::Image { .. } if message.role == Role::Tool => {}
            ContentPart::Image { media_type, data } => blocks.push(image(*media_type, data)),
            ContentPart::ReasoningState(item) => {
                let state = state(item)?;
                if message.role != Role::Assistant {
                    return Err(protocol(
                        "reasoning state is only sent on assistant messages",
                    ));
                }
                if states {
                    replayed.push(state);
                }
            }
        }
    }
    replayed.append(&mut blocks);
    let mut blocks = replayed;
    let role = match message.role {
        Role::Assistant => {
            blocks.extend(message.tool_calls.iter().map(|call| {
                json!({"type":"tool_use","id":call.id,"name":call.name,"input":input(&call.arguments)})
            }));
            "assistant"
        }
        Role::Tool => {
            let id = message
                .tool_call_id
                .as_ref()
                .ok_or_else(|| protocol("tool result requires tool_call_id"))?;
            let images: Vec<Value> = message
                .content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Image { media_type, data } => Some(image(*media_type, data)),
                    _ => None,
                })
                .collect();
            // Text alone keeps the plain string, so a request without images keeps its exact bytes.
            let content = if images.is_empty() {
                json!(plain(message))
            } else {
                let mut parts = vec![json!({"type":"text","text":plain(message)})];
                parts.extend(images);
                Value::Array(parts)
            };
            blocks.push(json!({"type":"tool_result","tool_use_id":id,"content":content}));
            "user"
        }
        _ => "user",
    };
    Ok((role, blocks))
}

/// An image block, its bytes in base64.
fn image(media_type: tqa_types::ImageMediaType, data: &[u8]) -> Value {
    json!({"type":"image","source":{"type":"base64","media_type":media_type.as_str(),"data":STANDARD.encode(data)}})
}

/// A recorded state as the block it was received as; only thinking blocks belong here.
fn state(item: &str) -> Result<Value, LlmError> {
    match serde_json::from_str::<Value>(item) {
        Ok(value)
            if matches!(
                value.get("type").and_then(Value::as_str),
                Some("thinking" | "redacted_thinking")
            ) =>
        {
            Ok(value)
        }
        _ => Err(protocol("reasoning state is not a thinking block")),
    }
}

/// The call's arguments as the object the protocol requires. Arguments that are not a JSON
/// object were already reported to the model as invalid (`ToolArgsInvalid`); replaying them as
/// `{}` only satisfies the protocol's shape, and the tool result still carries the error.
fn input(arguments: &str) -> Value {
    match serde_json::from_str::<Value>(&raw_arguments(arguments)) {
        Ok(value @ Value::Object(_)) => value,
        _ => Value::Object(Map::new()),
    }
}

/// The concatenated text of a message.
fn plain(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
