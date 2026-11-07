//! LLM-C-001/019 and LLM-U-017: wire shapes, credential placement and cache-stable prefixes.

use super::{ChatProvider, request};
use crate::{
    Auth, CompletionRequest, ContentPart, HttpConfig, LlmError, Message, Reasoning, Registry, Role,
    Route, StreamLimits, ToolCall, ToolSpec,
};
use proptest::prelude::*;
use secrecy::SecretString;
use serde_json::json;
use std::{error::Error, sync::Arc};
use tqa_testkit::{FakeClock, FakeSleeper};

fn route() -> Result<Route, Box<dyn Error>> {
    Registry::from_toml("[[routes]]\nname='test'\nprotocol='openai_chat'\nbase_url='https://models.example/v1/'\nmodel='test-model'")?.get("test").cloned().ok_or_else(|| "missing test route".into())
}

fn message(role: Role, text: String) -> Message {
    Message {
        role,
        content: vec![ContentPart::Text(text)],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}

#[test]
fn llm_c_019_credentials_have_exactly_one_destination() -> Result<(), Box<dyn Error>> {
    let secret = "synthetic value/+";
    for auth in [
        Auth::None,
        Auth::Bearer,
        Auth::Header {
            name: "x-credential".into(),
        },
        Auth::Query {
            name: "credential".into(),
        },
    ] {
        let mut route = route()?;
        route.auth = auth.clone();
        route.key_ref = Some("secrets/example".into());
        route
            .extra_headers
            .insert("x-feature".into(), "enabled".into());
        let clock = FakeClock::new();
        let provider = ChatProvider::new(
            route,
            Some(SecretString::from(secret)),
            HttpConfig::new(StreamLimits::RECOMMENDED),
            Arc::new(clock.clone()),
            Arc::new(FakeSleeper::new(clock)),
        )?;
        let request = provider
            .transport
            .request("chat/completions", reqwest::Method::POST)?
            .build()?;
        assert_eq!(request.url().path(), "/v1/chat/completions");
        assert_eq!(request.headers()["x-feature"], "enabled");
        let query: Vec<_> = request.url().query_pairs().collect();
        let bearer = request.headers().get("authorization");
        let custom = request.headers().get("x-credential");
        match auth {
            Auth::None => assert!(bearer.is_none() && custom.is_none() && query.is_empty()),
            Auth::Bearer => {
                let bearer = bearer.ok_or("missing bearer")?;
                assert_eq!(bearer.to_str()?, format!("Bearer {secret}"));
                assert!(bearer.is_sensitive() && custom.is_none() && query.is_empty());
            }
            Auth::Header { .. } => {
                let custom = custom.ok_or("missing custom credential")?;
                assert_eq!(custom.to_str()?, secret);
                assert!(custom.is_sensitive() && bearer.is_none() && query.is_empty());
            }
            Auth::Query { .. } => {
                assert_eq!(query, [("credential".into(), secret.into())]);
                assert!(bearer.is_none() && custom.is_none());
            }
        }
    }
    Ok(())
}

#[test]
fn llm_c_001_request_overrides_and_message_names() -> Result<(), Box<dyn Error>> {
    let mut route = route()?;
    route.limits.max_output = Some(20);
    route.reasoning = Some(Reasoning {
        field: "reasoning_effort".into(),
        allowed: vec!["low".into(), "high".into()],
        default: "low".into(),
        budget_tokens: None,
    });
    let mut named = message(Role::User, "a".into());
    named.content.push(ContentPart::Text("b".into()));
    named.name = Some("reader".into());
    let request = CompletionRequest {
        messages: vec![named],
        max_output: Some(10),
        temperature: Some(0.25),
        stop: vec!["end".into()],
        metadata: [("batch".into(), "test".into())].into(),
        reasoning_effort: Some("high".into()),
        ..CompletionRequest::default()
    };
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&request::encode(&route, &request)?)?,
        json!({"messages":[{"role":"user","content":"ab","name":"reader"}],"model":"test-model","stream":true,"stream_options":{"include_usage":true},"max_tokens":10,"temperature":0.25,"stop":["end"],"metadata":{"batch":"test"},"reasoning_effort":"high"})
    );
    Ok(())
}

#[test]
fn llm_c_001_invalid_requests_fail_before_transport() -> Result<(), Box<dyn Error>> {
    let mut route = route()?;
    assert!(matches!(
        request::encode(&route, &CompletionRequest::default()),
        Err(LlmError::Protocol(_))
    ));
    let mut request = CompletionRequest {
        messages: vec![message(Role::Tool, String::new())],
        ..CompletionRequest::default()
    };
    assert!(matches!(
        request::encode(&route, &request),
        Err(LlmError::Protocol(_))
    ));
    request.messages[0].role = Role::User;
    request.messages[0].tool_calls.push(ToolCall {
        id: "a".into(),
        name: "read".into(),
        arguments: "{}".into(),
    });
    assert!(matches!(
        request::encode(&route, &request),
        Err(LlmError::Protocol(_))
    ));
    request.messages[0].tool_calls.clear();
    request.temperature = Some(f32::NAN);
    assert!(matches!(
        request::encode(&route, &request),
        Err(LlmError::Protocol(_))
    ));
    request.temperature = None;
    route.native_tools = false;
    request.tools.push(ToolSpec {
        name: "read".into(),
        description: "Read".into(),
        parameters: json!({"type":"object"}),
        strict: true,
    });
    assert!(matches!(
        request::encode(&route, &request),
        Err(LlmError::Protocol(_))
    ));
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, rng_seed: proptest::test_runner::RngSeed::Fixed(5017), ..ProptestConfig::default() })]
    #[test]
    fn llm_u_017_property_system_and_tools_stay_byte_stable(
        system in "[^\\p{C}]{0,40}", tool in "[a-z_]{1,16}", users in proptest::collection::vec("[^\\p{C}]{0,20}", 20)
    ) {
        let route = route().map_err(|error| TestCaseError::fail(error.to_string()))?;
        // A strict-valid schema, so the route's default (auto) keeps `strict: true`.
        let parameters = json!({"type":"object","additionalProperties":false,"required":["path"],"properties":{"path":{"type":"string"}}});
        let tools = json!([{"type":"function","function":{"name":tool,"description":"Read","parameters":parameters,"strict":true}}]);
        let system_wire = json!({"content":system,"role":"system"});
        let prefix = format!("{{\"messages\":[{system_wire},");
        for user in users {
            let request = CompletionRequest {
                messages: vec![message(Role::System, system.clone()), message(Role::User, user.clone())],
                tools: vec![ToolSpec { name: tool.clone(), description: "Read".into(), parameters: parameters.clone(), strict: true }],
                ..CompletionRequest::default()
            };
            let encoded = request::encode(&route, &request)?;
            prop_assert!(encoded.starts_with(prefix.as_bytes()));
            let expected = json!({"messages":[system_wire,{"content":user,"role":"user"}],"model":"test-model","parallel_tool_calls":false,"stream":true,"stream_options":{"include_usage":true},"tools":tools});
            prop_assert_eq!(encoded, serde_json::to_vec(&expected)?);
        }
    }
}

/// Strict single-system chat templates (some open-weight models) refuse a system message anywhere but first.
#[test]
fn llm_u_033_leading_system_messages_merge_into_one_and_later_notices_go_as_user_text()
-> Result<(), Box<dyn Error>> {
    let route = route()?;
    let request = CompletionRequest {
        messages: vec![
            message(Role::System, "system prompt".into()),
            message(Role::System, "instructions".into()),
            message(Role::System, String::new()),
            message(Role::System, "tools".into()),
            message(Role::User, "task".into()),
            message(Role::System, "Environment changed to workstation.".into()),
            message(Role::User, "more".into()),
        ],
        ..CompletionRequest::default()
    };
    let encoded: serde_json::Value = serde_json::from_slice(&request::encode(&route, &request)?)?;
    assert_eq!(
        encoded["messages"],
        json!([
            {"role":"system","content":"system prompt\n\ninstructions\n\ntools"},
            {"role":"user","content":"task"},
            {"role":"user","content":format!("{}Environment changed to workstation.", request::NOTICE_PREFIX)},
            {"role":"user","content":"more"},
        ])
    );
    let roles: Vec<_> = encoded["messages"]
        .as_array()
        .ok_or("messages")?
        .iter()
        .map(|message| message["role"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        roles.iter().filter(|role| **role == "system").count(),
        1,
        "exactly one system message, and it is first: {roles:?}"
    );
    Ok(())
}

#[test]
fn llm_u_033_a_single_or_absent_system_message_is_sent_unchanged() -> Result<(), Box<dyn Error>> {
    let route = route()?;
    for (messages, expected) in [
        (
            vec![
                message(Role::System, "only".into()),
                message(Role::User, "u".into()),
            ],
            json!([{"role":"system","content":"only"},{"role":"user","content":"u"}]),
        ),
        (
            vec![
                message(Role::System, String::new()),
                message(Role::User, "u".into()),
            ],
            json!([{"role":"system","content":""},{"role":"user","content":"u"}]),
        ),
        (
            vec![message(Role::User, "u".into())],
            json!([{"role":"user","content":"u"}]),
        ),
        (
            // A system message carries text only; any other part is left out of the merged text.
            vec![
                Message {
                    content: vec![
                        ContentPart::Text("before ".into()),
                        ContentPart::Image {
                            media_type: tqa_types::ImageMediaType::Png,
                            data: vec![0x89, b'P', b'N', b'G'],
                        },
                        ContentPart::Text("after".into()),
                    ],
                    ..message(Role::System, String::new())
                },
                message(Role::User, "u".into()),
            ],
            json!([{"role":"system","content":"before after"},{"role":"user","content":"u"}]),
        ),
    ] {
        let request = CompletionRequest {
            messages,
            ..CompletionRequest::default()
        };
        let encoded: serde_json::Value =
            serde_json::from_slice(&request::encode(&route, &request)?)?;
        assert_eq!(encoded["messages"], expected);
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, rng_seed: proptest::test_runner::RngSeed::Fixed(5033), ..ProptestConfig::default() })]
    #[test]
    fn llm_u_033_property_the_merged_system_prefix_stays_byte_stable_across_steps(
        system in "[^\\p{C}]{1,40}", instructions in "[^\\p{C}]{0,40}", tools in "[^\\p{C}]{1,40}",
        turns in proptest::collection::vec("[^\\p{C}]{0,20}", 20)
    ) {
        let route = route().map_err(|error| TestCaseError::fail(error.to_string()))?;
        let slots = [
            message(Role::System, system.clone()),
            message(Role::System, instructions.clone()),
            message(Role::System, tools.clone()),
        ];
        let merged: Vec<&str> = [system.as_str(), instructions.as_str(), tools.as_str()]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect();
        let system_wire = json!({"content":merged.join("\n\n"),"role":"system"});
        let prefix = format!("{{\"messages\":[{system_wire},");
        let mut history = Vec::new();
        for turn in turns {
            history.push(message(Role::User, turn.clone()));
            history.push(message(Role::System, format!("notice {turn}")));
            let request = CompletionRequest {
                messages: slots.iter().cloned().chain(history.iter().cloned()).collect(),
                ..CompletionRequest::default()
            };
            let encoded = request::encode(&route, &request)?;
            prop_assert!(encoded.starts_with(prefix.as_bytes()));
        }
    }
}

/// LLM-U-043: each `chat.tool_call_extra` state goes back on its own call, by id; nothing is guessed.
#[test]
fn llm_u_043_tool_call_extras_go_back_on_their_calls() -> Result<(), Box<dyn Error>> {
    let signed = serde_json::json!({"google":{"thought_signature":"opaque-2"}});
    let item = |call_id: &str| {
        serde_json::json!({"type":"chat.tool_call_extra","call_id":call_id,"extra_content":signed})
            .to_string()
    };
    let calls = vec![
        ToolCall {
            id: "c1".into(),
            name: "a".into(),
            arguments: "{}".into(),
        },
        ToolCall {
            id: "c2".into(),
            name: "b".into(),
            arguments: "{}".into(),
        },
    ];
    let assistant = |content: Vec<ContentPart>| Message {
        role: Role::Assistant,
        content,
        tool_calls: calls.clone(),
        tool_call_id: None,
        name: None,
    };
    let encode = |content: Vec<ContentPart>| -> Result<serde_json::Value, Box<dyn Error>> {
        let request = CompletionRequest {
            messages: vec![message(Role::User, "hi".into()), assistant(content)],
            ..CompletionRequest::default()
        };
        Ok(serde_json::from_slice(&crate::encode_request(
            &route()?,
            &request,
        )?)?)
    };
    let body = encode(vec![ContentPart::ReasoningState(item("c2"))])?;
    let wire = &body["messages"][1]["tool_calls"];
    assert!(wire[0].get("extra_content").is_none(), "{wire}");
    assert_eq!(wire[1]["extra_content"], signed);
    // An item for a call not in the message, or of another type, is left out; the bytes are as without it.
    let plain = crate::encode_request(
        &route()?,
        &CompletionRequest {
            messages: vec![message(Role::User, "hi".into()), assistant(vec![])],
            ..CompletionRequest::default()
        },
    )?;
    for content in [
        vec![ContentPart::ReasoningState(item("c9"))],
        vec![ContentPart::ReasoningState(
            serde_json::json!({"type":"reasoning","encrypted_content":"x"}).to_string(),
        )],
    ] {
        let request = CompletionRequest {
            messages: vec![message(Role::User, "hi".into()), assistant(content)],
            ..CompletionRequest::default()
        };
        assert_eq!(crate::encode_request(&route()?, &request)?, plain);
    }
    Ok(())
}
