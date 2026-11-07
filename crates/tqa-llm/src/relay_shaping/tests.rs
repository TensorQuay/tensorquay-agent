use super::{read_usage, shape_request};
use crate::{Protocol, Registry, Route, Usage};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn route() -> Result<Route, Box<dyn std::error::Error>> {
    let registry = Registry::from_toml(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='pinned-model'\nstream_usage=true\nreasoning={field='thinking_mode',allowed=['low','high'],default='low'}\nextra_body={custom_setting={enabled=true}}\n",
    )?;
    Ok(registry.get("local").ok_or("missing route")?.clone())
}

#[test]
fn gw_i_039_relay_pins_model_and_route_fields_while_preserving_the_request() -> TestResult {
    let route = route()?;
    let mut body = json!({"model":"untrusted-model","messages":[{"role":"assistant","content":"hello","retained_signature":"opaque"}],"tools":[],"stream":true,"reasoning_effort":"high","thinking_mode":"untrusted","custom_setting":{"enabled":false},"stream_options":{"include_usage":false,"other":"keep"},"unknown":{"keep":true}});
    let messages = body["messages"].clone();
    shape_request(&route, &mut body)?;
    assert_eq!(body["model"], "pinned-model");
    assert_eq!(body["messages"], messages);
    assert_eq!(body["thinking_mode"], "high");
    assert!(body.get("reasoning_effort").is_none());
    assert_eq!(body["custom_setting"], json!({"enabled":true}));
    assert_eq!(
        body["stream_options"],
        json!({"include_usage":true,"other":"keep"})
    );
    assert_eq!(body["unknown"], json!({"keep":true}));
    assert_eq!(body["tools"], json!([]));
    assert_eq!(body["stream"], true);
    Ok(())
}

#[test]
fn gw_i_039_relay_applies_defaults_and_rejects_invalid_shapes_or_routes() -> TestResult {
    let mut route = route()?;
    let mut default = json!({"messages":[],"stream_options":null});
    shape_request(&route, &mut default)?;
    assert_eq!(default["thinking_mode"], "low");
    assert_eq!(default["stream_options"]["include_usage"], true);
    for mut body in [
        json!(null),
        json!([]),
        json!({"reasoning_effort":9}),
        json!({"reasoning_effort":"unknown"}),
        json!({"stream_options":"invalid"}),
    ] {
        assert!(shape_request(&route, &mut body).is_err());
    }
    route.reasoning = None;
    // Set false (a server that refuses the field): the request goes without `stream_options`, whatever it carried.
    route.stream_usage = Some(false);
    let mut body = json!({"reasoning_effort":"retain","stream_options":{"include_usage":true}});
    shape_request(&route, &mut body)?;
    assert_eq!(body["reasoning_effort"], "retain");
    assert_eq!(body.get("stream_options"), None);
    // The route is validated again: this one lacks the `max_output` a Messages route requires.
    route.protocol = Protocol::AnthropicMessages;
    assert!(
        matches!(shape_request(&route, &mut body), Err(crate::LlmError::Protocol(text)) if text.contains("max_output"))
    );
    route.protocol = Protocol::OpenAiChat;
    route
        .extra_body
        .insert("messages".into(), json!("must not replace"));
    assert!(shape_request(&route, &mut body).is_err());
    Ok(())
}

#[test]
fn gw_i_039_relay_reasoning_uses_the_exact_configured_name() -> TestResult {
    let mut route = route()?;
    let policy = route.reasoning.as_mut().ok_or("missing policy")?;
    policy.field = "reasoning_effort".into();
    let mut body = json!({"reasoning_effort":"high"});
    shape_request(&route, &mut body)?;
    assert_eq!(body["reasoning_effort"], "high");
    route.reasoning.as_mut().ok_or("missing policy")?.field = "reasoning.effort".into();
    shape_request(&route, &mut body)?;
    assert_eq!(body["reasoning.effort"], "high");
    assert!(body.get("reasoning_effort").is_none());
    Ok(())
}

#[test]
fn gw_i_039_usage_reuses_the_protocol_decoder_and_preserves_missing_counters() {
    let json = r#"{"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":4},"completion_tokens_details":{"reasoning_tokens":2}}}"#;
    assert_eq!(
        read_usage(Protocol::OpenAiChat, json),
        Some(Usage {
            prompt_tokens: 12,
            completion_tokens: 7,
            cached_tokens: Some(4),
            reasoning_tokens: Some(2)
        })
    );
    let missing = r#"{"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":0}}"#;
    assert_eq!(
        read_usage(Protocol::OpenAiChat, missing),
        Some(Usage {
            prompt_tokens: 1,
            completion_tokens: 0,
            cached_tokens: None,
            reasoning_tokens: None
        })
    );
    assert_eq!(read_usage(Protocol::AnthropicMessages, json), None);
    for data in [
        "",
        "[DONE]",
        "{}",
        r#"{"choices":[]}"#,
        r#"{"choices":[],"usage":{"prompt_tokens":-1,"completion_tokens":0}}"#,
    ] {
        assert_eq!(read_usage(Protocol::OpenAiChat, data), None);
    }
}

fn responses_route(extra: &str) -> Result<Route, Box<dyn std::error::Error>> {
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='resp'\nprotocol='openai_responses'\nbase_url='https://models.example/v1'\nmodel='pinned-model'\n{extra}"
    ))?;
    Ok(registry.get("resp").ok_or("missing route")?.clone())
}

#[test]
fn llm_c_024_relay_shapes_a_responses_request() -> TestResult {
    let route = responses_route(
        "reasoning={field='reasoning_effort',allowed=['low','high'],default='low'}\n\
         extra_body={reasoning={summary='auto'},service_tier='flex',text={verbosity='low'}}\n",
    )?;
    let input = json!([{"type":"message","role":"user","content":"hi"},
        {"type":"function_call","call_id":"c","name":"n","arguments":"{}"}]);
    let mut body = json!({"model":"untrusted","input":input,"stream":false,"store":true,
        "reasoning":{"effort":"high"},"text":{"format":{"type":"text"}},"unknown":{"keep":true}});
    shape_request(&route, &mut body)?;
    assert_eq!(
        body,
        json!({"model":"pinned-model","input":input,"stream":true,"store":false,
            "reasoning":{"effort":"high","summary":"auto"},"service_tier":"flex",
            "text":{"format":{"type":"text"},"verbosity":"low"},"unknown":{"keep":true}})
    );
    let mut default = json!({"input":[]});
    shape_request(&route, &mut default)?;
    assert_eq!(
        default["reasoning"],
        json!({"effort":"low","summary":"auto"})
    );
    for mut invalid in [
        json!({"input":[],"reasoning":{"effort":"extreme"}}),
        json!({"input":[],"reasoning":{"effort":3}}),
        json!({"input":[],"reasoning":"high"}),
        json!([]),
    ] {
        assert!(shape_request(&route, &mut invalid).is_err(), "{invalid}");
    }
    let plain = responses_route("")?;
    let mut untouched = json!({"input":[],"reasoning":{"effort":"medium"}});
    shape_request(&plain, &mut untouched)?;
    assert_eq!(untouched["reasoning"], json!({"effort":"medium"}));
    assert_eq!(untouched["store"], false);
    Ok(())
}

#[test]
fn llm_c_024_responses_routes_reserve_their_own_fields() {
    for extra in [
        "extra_body={input='x'}",
        "extra_body={store=true}",
        "extra_body={previous_response_id='resp_1'}",
        "extra_body={max_output_tokens=9}",
        "extra_body={reasoning={effort='high'}}",
        "extra_body={reasoning='high'}",
        "extra_body={include=['reasoning.encrypted_content']}",
    ] {
        assert!(responses_route(&format!("{extra}\n")).is_err(), "{extra}");
    }
    assert!(responses_route("extra_body={reasoning={summary='auto'}}\n").is_ok());
}

#[test]
fn llm_c_024_relay_reads_usage_from_responses_terminal_events() {
    let completed = r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":12,"input_tokens_details":{"cached_tokens":4},"output_tokens":7,"output_tokens_details":{"reasoning_tokens":2}}}}"#;
    assert_eq!(
        read_usage(Protocol::OpenAiResponses, completed),
        Some(Usage {
            prompt_tokens: 12,
            completion_tokens: 7,
            cached_tokens: Some(4),
            reasoning_tokens: Some(2)
        })
    );
    for kind in ["response.incomplete", "response.failed"] {
        let event = format!(
            r#"{{"type":"{kind}","response":{{"usage":{{"input_tokens":1,"output_tokens":0}}}}}}"#
        );
        assert_eq!(
            read_usage(Protocol::OpenAiResponses, &event),
            Some(Usage {
                prompt_tokens: 1,
                completion_tokens: 0,
                cached_tokens: None,
                reasoning_tokens: None
            })
        );
    }
    for data in [
        "",
        "{}",
        r#"{"type":"response.output_text.delta","delta":"x"}"#,
        r#"{"type":"response.completed","response":{"status":"completed"}}"#,
        r#"{"type":"response.created","response":{"usage":{"input_tokens":1,"output_tokens":1}}}"#,
    ] {
        assert_eq!(read_usage(Protocol::OpenAiResponses, data), None, "{data}");
    }
    assert_eq!(read_usage(Protocol::OpenAiChat, completed), None);
}

/// A route of `protocol` with the given tool-schema settings.
fn with_settings(
    protocol: &str,
    strict: &str,
    keywords: &str,
) -> Result<Route, Box<dyn std::error::Error>> {
    let max_output = if protocol == "anthropic_messages" {
        "max_output=4096\n"
    } else {
        ""
    };
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='r'\nprotocol='{protocol}'\nbase_url='https://models.example/v1'\nmodel='m'\n\
         {max_output}strict='{strict}'\nschema_keywords='{keywords}'\n"
    ))?;
    Ok(registry.get("r").ok_or("route")?.clone())
}

#[test]
fn llm_c_032_relay_shaping_applies_the_tool_schema_settings() -> TestResult {
    // A strict-valid schema with keywords basic drops (pattern, additionalProperties).
    let parameters = json!({"type":"object","additionalProperties":false,"required":["q"],
        "properties":{"q":{"type":"string","pattern":"^a"}}});
    let basic = json!({"type":"object","required":["q"],"properties":{"q":{"type":"string"}}});
    // Chat: the definition under `function`.
    let mut chat = json!({"model":"x","messages":[],"tools":[{"type":"function","function":{"name":"t","parameters":parameters,"strict":true}}]});
    shape_request(&with_settings("openai_chat", "off", "basic")?, &mut chat)?;
    assert_eq!(chat["tools"][0]["function"]["strict"], false);
    assert_eq!(chat["tools"][0]["function"]["parameters"], basic);
    // Responses: the definition at the top level.
    let mut responses = json!({"model":"x","input":[],"tools":[{"type":"function","name":"t","parameters":parameters,"strict":false}]});
    shape_request(
        &with_settings("openai_responses", "on", "full")?,
        &mut responses,
    )?;
    assert_eq!(responses["tools"][0]["strict"], true);
    assert_eq!(responses["tools"][0]["parameters"], parameters);
    // Auto recomputes on the schema as it goes out.
    let mut auto = json!({"model":"x","messages":[],"tools":[{"type":"function","function":{"name":"t","parameters":{"type":"object","properties":{}},"strict":true}}]});
    shape_request(&with_settings("openai_chat", "auto", "full")?, &mut auto)?;
    assert_eq!(
        auto["tools"][0]["function"]["strict"], false,
        "an open object is not strict-valid"
    );
    // Messages: tools are left as they are.
    let messages_tools = json!([{"name":"t","input_schema":parameters}]);
    let mut messages = json!({"model":"x","messages":[],"max_tokens":16,"tools":messages_tools});
    shape_request(
        &with_settings("anthropic_messages", "off", "basic")?,
        &mut messages,
    )?;
    assert_eq!(messages["tools"], messages_tools);
    Ok(())
}

#[test]
fn llm_c_032_the_adapters_apply_the_settings_when_they_encode() -> TestResult {
    let request = crate::CompletionRequest {
        tools: vec![crate::ToolSpec {
            name: "t".into(),
            description: "d".into(),
            parameters: json!({"type":"object","additionalProperties":false,"required":["q"],
                "properties":{"q":{"type":"string","maxLength":9}}}),
            strict: true,
        }],
        messages: vec![crate::Message {
            role: crate::Role::User,
            content: vec![crate::ContentPart::Text("hi".into())],
            tool_calls: vec![],
            tool_call_id: None,
            name: None,
        }],
        ..crate::CompletionRequest::default()
    };
    let chat: serde_json::Value = serde_json::from_slice(&crate::encode_request(
        &with_settings("openai_chat", "off", "basic")?,
        &request,
    )?)?;
    assert_eq!(chat["tools"][0]["function"]["strict"], false);
    assert!(
        chat["tools"][0]["function"]["parameters"]["properties"]["q"]
            .get("maxLength")
            .is_none()
    );
    let responses: serde_json::Value = serde_json::from_slice(&crate::encode_request(
        &with_settings("openai_responses", "off", "full")?,
        &request,
    )?)?;
    assert_eq!(responses["tools"][0]["strict"], false);
    assert_eq!(
        responses["tools"][0]["parameters"]["properties"]["q"]["maxLength"],
        9
    );
    Ok(())
}
