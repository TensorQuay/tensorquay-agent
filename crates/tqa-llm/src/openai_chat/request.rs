//! Deterministic request shaping for streaming chat completions.

use crate::{
    CompletionRequest, ContentPart, LlmError, Message, Role, Route,
    request::{check_catalog, leading_system, merge_absent, plain_text, raw_arguments},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

pub(super) use crate::request::NOTICE_PREFIX;

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
    let messages = single_system(&request.messages, route.vision)?;
    let mut body = json!({"model":route.model,"messages":messages,"stream":true});
    if route.stream_usage() {
        body["stream_options"] = json!({"include_usage":true});
    }
    if !request.tools.is_empty() {
        if !route.native_tools && !request.declare_tools {
            return Err(LlmError::Protocol(
                "route does not support native tools".into(),
            ));
        }
        body["tools"] = request.tools.iter().map(|tool| json!({"type":"function","function":{"name":tool.name,"description":tool.description,"parameters":tool.parameters,"strict":tool.strict}})).collect();
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
        body[&reasoning.field] = json!(effort);
    }
    if let Some(maximum) = request.max_output.or(route.limits.max_output) {
        body["max_tokens"] = json!(maximum);
    }
    if let Some(temperature) = request.temperature {
        if !temperature.is_finite() {
            return Err(LlmError::Protocol("temperature must be finite".into()));
        }
        body["temperature"] = json!(temperature);
    }
    if !request.stop.is_empty() {
        body["stop"] = json!(request.stop);
    }
    if !request.metadata.is_empty() {
        body["metadata"] = json!(request.metadata);
    }
    merge_absent(&mut body, &route.extra_body);
    serde_json::to_vec(&body).map_err(|error| LlmError::Protocol(crate::redact(&error.to_string())))
}

/// Strict single-system chat templates (some open-weight models) refuse a system message anywhere but first, and every
/// compatible server accepts one. So the leading system messages go as one, joined by blank lines in order (a
/// byte-stable prefix, so prompt caching holds), and a later system notice goes as user text with a marker.
fn single_system(messages: &[Message], vision: bool) -> Result<Vec<Value>, LlmError> {
    let (system, rest) = leading_system(messages);
    let mut wire = Vec::with_capacity(rest.len() + 1);
    if !system.is_empty() {
        let texts: Vec<String> = system.into_iter().filter(|text| !text.is_empty()).collect();
        wire.push(json!({"role":Role::System,"content":texts.join("\n\n")}));
    }
    // A tool result's image goes as a user message after the run of tool results, which must stay together.
    let mut shown = Vec::new();
    for item in rest {
        if item.role != Role::Tool {
            wire.append(&mut shown);
        }
        if item.role == Role::System {
            wire.push(
                json!({"role":Role::User,"content":format!("{NOTICE_PREFIX}{}", plain_text(item))}),
            );
        } else if let Some((result, images)) = crate::request::tool_images(item) {
            wire.push(message(&result, vision)?);
            shown.push(message(&images, vision)?);
        } else {
            wire.push(message(item, vision)?);
        }
    }
    wire.append(&mut shown);
    Ok(wire)
}

fn message(message: &Message, vision: bool) -> Result<Value, LlmError> {
    let mut value = json!({"role":message.role,"content":content(&message.content, vision)?});
    if let Some(name) = &message.name {
        value["name"] = json!(name);
    }
    if message.role == Role::Tool && message.tool_call_id.is_none() {
        return Err(LlmError::Protocol(
            "tool result requires tool_call_id".into(),
        ));
    }
    if let Some(id) = &message.tool_call_id {
        value["tool_call_id"] = json!(id);
    }
    if !message.tool_calls.is_empty() {
        if message.role != Role::Assistant {
            return Err(LlmError::Protocol(
                "only assistant messages may contain tool calls".into(),
            ));
        }
        let extras = tool_call_extras(&message.content);
        value["tool_calls"] = message
            .tool_calls
            .iter()
            .map(|call| {
                let mut wire = json!({"id":call.id,"type":"function","function":{"name":call.name,"arguments":raw_arguments(&call.arguments)}});
                if let Some(extra) = extras.get(call.id.as_str()) {
                    wire["extra_content"] = extra.clone();
                }
                wire
            })
            .collect();
    }
    Ok(value)
}

/// The opaque extras recorded for this message's tool calls, by call id (ADR-0027); other states are not Chat's.
fn tool_call_extras(parts: &[ContentPart]) -> std::collections::BTreeMap<String, Value> {
    parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::ReasoningState(item) => serde_json::from_str::<Value>(item).ok(),
            _ => None,
        })
        .filter(|item| {
            item["type"] == crate::CHAT_TOOL_CALL_EXTRA && item["extra_content"].is_object()
        })
        .filter_map(|item| {
            Some((
                item["call_id"].as_str()?.to_owned(),
                item["extra_content"].clone(),
            ))
        })
        .collect()
}

/// Text-only content stays one string, so requests without images keep their exact bytes;
/// content with an image becomes an ordered array of text and data-URL image parts. Opaque reasoning states never go in
/// content: a Chat tool call's extra goes back on its call (`tool_call_extras`), and other states are left out.
fn content(parts: &[ContentPart], vision: bool) -> Result<Value, LlmError> {
    let has_image = parts
        .iter()
        .any(|part| matches!(part, ContentPart::Image { .. }));
    if !has_image {
        let text: String = parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text(text) => Some(text.as_str()),
                ContentPart::Image { .. } | ContentPart::ReasoningState(_) => None,
            })
            .collect();
        return Ok(json!(text));
    }
    if !vision {
        return Err(LlmError::Protocol(
            "route does not accept image input".into(),
        ));
    }
    Ok(parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text(text) => Some(json!({"type":"text","text":text})),
            ContentPart::Image { media_type, data } => {
                let url = format!(
                    "data:{};base64,{}",
                    media_type.as_str(),
                    STANDARD.encode(data)
                );
                Some(json!({"type":"image_url","image_url":{"url":url}}))
            }
            ContentPart::ReasoningState(_) => None,
        })
        .collect())
}

#[cfg(test)]
mod tests;
