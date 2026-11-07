//! Deterministic request shaping for streaming responses, built from our own history.

use crate::{
    CompletionRequest, ContentPart, LlmError, Message, Role, Route,
    request::{
        NOTICE_PREFIX, check_catalog, leading_system, merge_absent, plain_text, raw_arguments,
    },
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

pub(super) fn encode(route: &Route, request: &CompletionRequest) -> Result<Vec<u8>, LlmError> {
    // The route's tool-schema settings apply before the budget, so it measures what goes out.
    let shaped;
    let request = if request.tools.is_empty() {
        request
    } else {
        shaped = CompletionRequest {
            tools: crate::tool_schemas::shape_specs(
                route.strict_tools(),
                route.schema_keywords(),
                &request.tools,
            ),
            ..request.clone()
        };
        &shaped
    };
    check_catalog(&request.tools)?;
    if request.messages.is_empty() {
        return Err(LlmError::Protocol(
            "completion requires at least one message".into(),
        ));
    }
    if !request.stop.is_empty() {
        return Err(LlmError::Protocol(
            "route protocol does not support stop sequences".into(),
        ));
    }
    // The leading system messages go as the top-level `instructions`, joined as on Chat (LLM-U-033): some servers
    // refuse a `system` input message, and some templates refuse `developer` (LLM-U-037). A later system message (a
    // notice) goes as user text.
    let (system, rest) = leading_system(&request.messages);
    let instructions: Vec<String> = system.into_iter().filter(|text| !text.is_empty()).collect();
    let input = input_items(rest, route.vision)?;
    let mut body = json!({"model":route.model,"input":input,"stream":true,"store":false});
    if !instructions.is_empty() {
        body["instructions"] = json!(instructions.join("\n\n"));
    }
    if route.reasoning.is_some() {
        body["include"] = json!(["reasoning.encrypted_content"]);
    }
    if !request.tools.is_empty() {
        if !route.native_tools {
            return Err(LlmError::Protocol(
                "route does not support native tools".into(),
            ));
        }
        body["tools"] = request.tools.iter().map(|tool| json!({"type":"function","name":tool.name,"description":tool.description,"parameters":tool.parameters,"strict":tool.strict})).collect();
        body["parallel_tool_calls"] = json!(route.parallel_tools);
    }
    if let Some(reasoning) = &route.reasoning {
        let effort = request
            .reasoning_effort
            .as_ref()
            .unwrap_or(&reasoning.default);
        if !reasoning.allowed.contains(effort) {
            return Err(LlmError::Protocol(
                "reasoning effort is not allowed by the route".into(),
            ));
        }
        body["reasoning"] = json!({"effort":effort});
    }
    if let Some(maximum) = request.max_output.or(route.limits.max_output) {
        body["max_output_tokens"] = json!(maximum);
    }
    if let Some(temperature) = request.temperature {
        if !temperature.is_finite() {
            return Err(LlmError::Protocol("temperature must be finite".into()));
        }
        body["temperature"] = json!(temperature);
    }
    if !request.metadata.is_empty() {
        body["metadata"] = json!(request.metadata);
    }
    merge_absent(&mut body, &route.extra_body);
    serde_json::to_vec(&body).map_err(|error| LlmError::Protocol(crate::redact(&error.to_string())))
}

/// The input items of the messages after the leading system ones. A later system message (a notice) goes as user text,
/// and a tool result's image as a user message after the run of tool outputs, as on Chat.
fn input_items(rest: &[Message], vision: bool) -> Result<Vec<Value>, LlmError> {
    let mut input = Vec::new();
    let mut shown = Vec::new();
    for message in rest {
        if message.role != Role::Tool {
            input.append(&mut shown);
        }
        if message.role == Role::System {
            let text = format!("{NOTICE_PREFIX}{}", plain_text(message));
            input.push(json!({"type":"message","role":"user","content":text}));
        } else if let Some((result, images)) = crate::request::tool_images(message) {
            items(&result, vision, &mut input)?;
            items(&images, vision, &mut shown)?;
        } else {
            items(message, vision, &mut input)?;
        }
    }
    input.append(&mut shown);
    Ok(input)
}

/// Appends a message's input items: a message item for its content, then one `function_call`
/// item per call; a tool result is one `function_call_output` item. `name` has no item field.
fn items(message: &Message, vision: bool, input: &mut Vec<Value>) -> Result<(), LlmError> {
    if !message.tool_calls.is_empty() && message.role != Role::Assistant {
        return Err(LlmError::Protocol(
            "only assistant messages may contain tool calls".into(),
        ));
    }
    let (states, content) = split_states(&message.content)?;
    if !states.is_empty() && message.role != Role::Assistant {
        return Err(LlmError::Protocol(
            "reasoning state is only sent on assistant messages".into(),
        ));
    }
    input.extend(states);
    let role = match message.role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => {
            let id = message
                .tool_call_id
                .as_ref()
                .ok_or_else(|| LlmError::Protocol("tool result requires tool_call_id".into()))?;
            let output = text_only(&content)
                .ok_or_else(|| LlmError::Protocol("tool result must be text".into()))?;
            input.push(json!({"type":"function_call_output","call_id":id,"output":output}));
            return Ok(());
        }
    };
    if let Some(text) = text_only(&content) {
        if message.role != Role::Assistant || !text.is_empty() {
            input.push(json!({"type":"message","role":role,"content":text}));
        }
    } else {
        if !vision {
            return Err(LlmError::Protocol(
                "route does not accept image input".into(),
            ));
        }
        if message.role != Role::User {
            return Err(LlmError::Protocol(
                "image input is only sent in user messages".into(),
            ));
        }
        input.push(json!({"type":"message","role":role,"content":parts(&content)}));
    }
    for call in &message.tool_calls {
        input.push(json!({"type":"function_call","call_id":call.id,"name":call.name,"arguments":raw_arguments(&call.arguments)}));
    }
    Ok(())
}

/// Separates the recorded reasoning items, replayed unchanged as input items, from the content.
fn split_states(parts: &[ContentPart]) -> Result<(Vec<Value>, Vec<ContentPart>), LlmError> {
    let mut states = Vec::new();
    let mut content = Vec::new();
    for part in parts {
        match part {
            ContentPart::ReasoningState(item) => match serde_json::from_str::<Value>(item) {
                Ok(value @ Value::Object(_)) => states.push(value),
                _ => {
                    return Err(LlmError::Protocol(
                        "reasoning state is not a JSON object".into(),
                    ));
                }
            },
            other => content.push(other.clone()),
        }
    }
    Ok((states, content))
}

/// The concatenated text, or `None` when the content holds an image.
fn text_only(parts: &[ContentPart]) -> Option<String> {
    parts
        .iter()
        .map(|part| match part {
            ContentPart::Text(text) => Some(text.as_str()),
            ContentPart::Image { .. } | ContentPart::ReasoningState(_) => None,
        })
        .collect()
}

/// Message content parts; reasoning states were already moved to their own input items.
fn parts(parts: &[ContentPart]) -> Vec<Value> {
    parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text(text) => Some(json!({"type":"input_text","text":text})),
            ContentPart::Image { media_type, data } => {
                let url = format!(
                    "data:{};base64,{}",
                    media_type.as_str(),
                    STANDARD.encode(data)
                );
                Some(json!({"type":"input_image","image_url":url}))
            }
            ContentPart::ReasoningState(_) => None,
        })
        .collect()
}
