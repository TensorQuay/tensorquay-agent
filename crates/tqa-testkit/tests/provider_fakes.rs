//! TK-U-016: the provider fakes' rules. Each rule refuses a request that breaks it with the provider's status and error
//! body shape, and accepts one that meets it; a refusal consumes no script; every rule names its source.
use serde_json::{Value, json};
use std::{collections::BTreeSet, error::Error, sync::Arc};
use tqa_testkit::{
    FakeClock, FakeModelServer, FakeProfile, FakeSleeper, Script, Scripted, Source,
    THOUGHT_SIGNATURE_PATH, thought_signature,
};

type TestResult = Result<(), Box<dyn Error>>;

/// A request the profile accepts: the shape our adapters send after step 1.
fn conforming(profile: FakeProfile) -> Value {
    match profile {
        FakeProfile::OpenaiResponses | FakeProfile::OpenaiResponsesBackend => json!({
            "model": "m", "stream": true, "store": false,
            "instructions": "Be brief.",
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [{"type":"function","name":"read_file","description":"Read","strict":true,
                "parameters":{"type":"object","properties":{"path":{"type":"string"}},
                    "required":["path"],"additionalProperties":false}}]
        }),
        FakeProfile::DeepseekChat => json!({
            "model": "m", "stream": true,
            "messages": [{"role":"system","content":"Be brief."},{"role":"user","content":"hi"}],
            "tools": [{"type":"function","function":{"name":"read_file","description":"Read","strict":true,
                "parameters":{"type":"object","properties":{"path":{"type":"string"}},
                    "required":["path"],"additionalProperties":false}}}]
        }),
        // The basic keyword subset, as a route with schema_keywords = basic sends it.
        FakeProfile::GeminiOpenaiCompat => json!({
            "model": "m", "stream": true,
            "messages": [{"role":"system","content":"Be brief."},{"role":"user","content":"hi"}],
            "tools": [{"type":"function","function":{"name":"read_file","description":"Read",
                "parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}]
        }),
        FakeProfile::AnthropicMessages => json!({
            "model": "m", "stream": true, "max_tokens": 1000,
            "system": [{"type":"text","text":"Be brief."}],
            "messages": [{"role":"user","content":[{"type":"text","text":"hi"}]}],
            "tools": [{"name":"read_file","description":"Read",
                "input_schema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}]
        }),
    }
}

/// A rule's name and a change to a conforming request that breaks it.
type Case = (&'static str, fn(&mut Value));

/// The strict-schema cases, shared by both Responses profiles.
const STRICT: [Case; 4] = [
    ("strict_closed_objects", |body| {
        if let Some(schema) = body["tools"][0]["parameters"].as_object_mut() {
            schema.remove("additionalProperties");
        }
    }),
    ("strict_all_required", |body| {
        body["tools"][0]["parameters"]["properties"]["depth"] = json!({"type":"integer"});
    }),
    ("strict_supported_keywords", |body| {
        body["tools"][0]["parameters"]["properties"]["path"] = json!({"allOf":[{"type":"string"}]});
    }),
    ("strict_object_root", |body| {
        body["tools"][0]["parameters"] = json!({"anyOf":[{"type":"object","properties":{},
            "required":[],"additionalProperties":false}]});
    }),
];

/// One case per rule.
fn violations(profile: FakeProfile) -> Vec<Case> {
    let mut cases: Vec<Case> = match profile {
        FakeProfile::OpenaiResponses => vec![("tool_name_pattern", |body| {
            body["tools"][0]["name"] = json!("read file");
        })],
        FakeProfile::OpenaiResponsesBackend => vec![
            ("no_system_input", |body| {
                body["input"] = json!([{"type":"message","role":"system","content":"s"},
                    {"type":"message","role":"user","content":"hi"}]);
            }),
            ("unsupported_parameters", |body| {
                body["metadata"] = json!({"run":"r"});
            }),
        ],
        FakeProfile::DeepseekChat => vec![
            ("known_roles", |body| {
                body["messages"][0]["role"] = json!("developer");
            }),
            ("tool_limits", |body| {
                body["tools"][0]["function"]["name"] = json!("read file");
            }),
            ("strict_closed_objects", |body| {
                if let Some(schema) = body["tools"][0]["function"]["parameters"].as_object_mut() {
                    schema.remove("additionalProperties");
                }
            }),
            ("strict_all_required", |body| {
                body["tools"][0]["function"]["parameters"]["properties"]["depth"] =
                    json!({"type":"integer"});
            }),
        ],
        FakeProfile::GeminiOpenaiCompat => vec![
            ("thought_signature", |body| *body = round_trip(None)),
            ("known_roles", |body| {
                body["messages"][0]["role"] = json!("developer");
            }),
            ("schema_keywords", |body| {
                body["tools"][0]["function"]["parameters"]["additionalProperties"] = json!(false);
            }),
        ],
        FakeProfile::AnthropicMessages => vec![
            ("max_tokens_required", |body| {
                if let Some(body) = body.as_object_mut() {
                    body.remove("max_tokens");
                }
            }),
            ("no_system_role", |body| {
                body["messages"] =
                    json!([{"role":"system","content":"s"},{"role":"user","content":"hi"}]);
            }),
            ("system_text_non_empty", |body| {
                body["system"] =
                    json!([{"type":"text","text":"Be brief."},{"type":"text","text":""}]);
            }),
            ("first_turn_user", |body| {
                body["messages"] =
                    json!([{"role":"assistant","content":"a"},{"role":"user","content":"hi"}]);
            }),
            ("tool_name_pattern", |body| {
                body["tools"][0]["name"] = json!("read.file");
            }),
            ("input_schema_object", |body| {
                body["tools"][0]["input_schema"] = json!({"type":"string"});
            }),
            ("thinking_temperature", |body| {
                body["thinking"] = json!({"type":"enabled","budget_tokens":1024});
                body["temperature"] = json!(0.5);
            }),
        ],
    };
    if matches!(
        profile,
        FakeProfile::OpenaiResponses | FakeProfile::OpenaiResponsesBackend
    ) {
        cases.extend(STRICT);
    }
    cases
}

const PROFILES: [FakeProfile; 5] = [
    FakeProfile::OpenaiResponses,
    FakeProfile::OpenaiResponsesBackend,
    FakeProfile::AnthropicMessages,
    FakeProfile::DeepseekChat,
    FakeProfile::GeminiOpenaiCompat,
];

#[test]
fn tk_u_016_every_rule_refuses_its_violation_and_accepts_the_conforming_request() -> TestResult {
    for profile in PROFILES {
        let base = conforming(profile);
        assert_eq!(
            profile.check(profile.path(), &base),
            None,
            "{profile:?} accepts the conforming request"
        );
        for (name, breaks) in violations(profile) {
            let mut body = base.clone();
            breaks(&mut body);
            let refusal = profile
                .check(profile.path(), &body)
                .ok_or_else(|| format!("{profile:?} accepted a request breaking {name}"))?;
            assert_eq!(
                refusal.rule, name,
                "{profile:?}: the refusal names the rule it broke"
            );
            assert!(
                (400..500).contains(&refusal.status),
                "{profile:?} {name}: {}",
                refusal.status
            );
            let error: Value = serde_json::from_str(&refusal.body)?;
            assert!(
                profile
                    .error_message(&error)
                    .is_some_and(|message| !message.is_empty()),
                "{profile:?} {name}: the body has the provider's shape: {error}"
            );
        }
    }
    Ok(())
}

#[test]
fn tk_u_016_every_rule_has_a_case_and_names_its_source() {
    for profile in PROFILES {
        let rules: BTreeSet<_> = profile.rules().iter().map(|rule| rule.name).collect();
        let cases: BTreeSet<_> = violations(profile)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            rules, cases,
            "{profile:?}: a self-test per rule, and no test for a rule that doesn't exist"
        );
        for rule in profile.rules() {
            let source = match rule.source {
                Source::Documented(text) | Source::Observed(text) => text,
            };
            assert!(
                !source.trim().is_empty(),
                "{profile:?} {}: source named",
                rule.name
            );
        }
    }
}

#[test]
fn tk_u_016_the_backend_refuses_a_system_input_message_as_observed() -> TestResult {
    let profile = FakeProfile::OpenaiResponsesBackend;
    let mut body = conforming(profile);
    body["input"] = json!([{"type":"message","role":"system","content":"s"},
        {"type":"message","role":"user","content":"hi"}]);
    let refusal = profile.check(profile.path(), &body).ok_or("accepted")?;
    assert_eq!(refusal.status, 400);
    let error: Value = serde_json::from_str(&refusal.body)?;
    assert_eq!(
        profile.error_message(&error),
        Some("System messages are not allowed".into())
    );
    // A developer message and the top-level field are fine.
    body["input"][0]["role"] = json!("developer");
    assert_eq!(profile.check(profile.path(), &body), None);
    Ok(())
}

#[test]
fn tk_u_016_a_strict_tool_is_checked_but_a_non_strict_one_is_not() {
    let profile = FakeProfile::OpenaiResponses;
    let mut body = conforming(profile);
    body["tools"][0]["parameters"] = json!({"type":"object","properties":{"a":{"type":"string"}}});
    assert!(
        profile.check(profile.path(), &body).is_some(),
        "strict: open object refused"
    );
    body["tools"][0]["strict"] = json!(false);
    assert_eq!(
        profile.check(profile.path(), &body),
        None,
        "not strict: accepted"
    );
}

#[test]
fn tk_u_016_a_request_on_another_protocols_path_is_not_found() -> TestResult {
    let profile = FakeProfile::OpenaiResponses;
    let refusal = profile
        .check("/v1/chat/completions", &conforming(profile))
        .ok_or("accepted on the wrong path")?;
    assert_eq!(refusal.status, 404);
    Ok(())
}

#[tokio::test]
async fn tk_u_016_a_refusal_consumes_no_script() -> TestResult {
    let profile = FakeProfile::OpenaiResponsesBackend;
    let script = Script {
        header_delay: std::time::Duration::ZERO,
        events: vec![
            Scripted::Text("ok".into()),
            Scripted::Finish(tqa_llm::FinishReason::Stop),
        ],
    };
    let server = FakeModelServer::start_as(
        profile,
        vec![script],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    )
    .await?;
    let client = reqwest::Client::new();
    let url = format!("{}/responses", server.base_url());
    let mut bad = conforming(profile);
    bad["input"][0]["role"] = json!("system");
    let refused = client.post(&url).json(&bad).send().await?;
    assert_eq!(refused.status().as_u16(), 400);
    assert_eq!(
        refused
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
    let accepted = client.post(&url).json(&conforming(profile)).send().await?;
    assert_eq!(
        accepted.status().as_u16(),
        200,
        "the script was still queued"
    );
    assert!(accepted.text().await?.contains("ok"));
    assert_eq!(server.requests().len(), 2, "both requests are recorded");
    Ok(())
}

#[test]
fn tk_u_016_the_backends_strict_refusal_is_worded_as_observed() -> TestResult {
    let profile = FakeProfile::OpenaiResponsesBackend;
    let mut body = conforming(profile);
    // The observed case: edit_file's edits items with an optional replace_all.
    body["tools"][0] = json!({"type":"function","name":"edit_file","description":"Edit","strict":true,
        "parameters":{"type":"object","additionalProperties":false,"required":["path","edits"],"properties":{
            "path":{"type":"string"},
            "edits":{"type":"array","items":{"type":"object","additionalProperties":false,
                "required":["old_text","new_text"],"properties":{
                    "old_text":{"type":"string"},"new_text":{"type":"string"},"replace_all":{"type":"boolean"}}}}}}});
    let refusal = profile.check(profile.path(), &body).ok_or("accepted")?;
    let error: Value = serde_json::from_str(&refusal.body)?;
    assert_eq!(
        error["detail"],
        "Invalid schema for function 'edit_file': In context=('properties', 'edits', 'items'), 'required' is required to be supplied and to be an array including every key in properties. Missing 'replace_all'."
    );
    Ok(())
}

#[test]
fn tk_u_016_the_compatible_endpoint_refuses_a_nested_unsupported_keyword() -> TestResult {
    let profile = FakeProfile::GeminiOpenaiCompat;
    let mut body = conforming(profile);
    body["tools"][0]["function"]["parameters"]["properties"]["mode"] = json!({"const":"fast"});
    let refusal = profile.check(profile.path(), &body).ok_or("accepted")?;
    let error: Value = serde_json::from_str(&refusal.body)?;
    assert_eq!(error[0]["error"]["status"], "INVALID_ARGUMENT");
    assert_eq!(error[0]["error"]["code"], 400);
    assert!(
        profile
            .error_message(&error)
            .is_some_and(|message| message.contains("Unknown name \"const\""))
    );
    Ok(())
}

#[tokio::test]
async fn tk_u_016_the_compatible_endpoint_streams_tool_calls_without_an_index() -> TestResult {
    let profile = FakeProfile::GeminiOpenaiCompat;
    let script = Script {
        header_delay: std::time::Duration::ZERO,
        events: vec![Scripted::ToolCall {
            name: "read_file".into(),
            arguments_json: r#"{"path":"a"}"#.into(),
            split_into: 2,
        }],
    };
    let server = FakeModelServer::start_as(
        profile,
        vec![script],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    )
    .await?;
    let text = reqwest::Client::new()
        .post(format!("{}/chat/completions", server.base_url()))
        .json(&conforming(profile))
        .send()
        .await?
        .text()
        .await?;
    let calls: Vec<Value> = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .flat_map(|chunk| {
            chunk["choices"][0]["delta"]["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(calls.len(), 2, "both parts of the call streamed: {text}");
    assert!(
        calls.iter().all(|call| call.get("index").is_none()),
        "as observed live: {calls:?}"
    );
    assert_eq!(calls[0]["function"]["name"], "read_file");
    Ok(())
}

/// A compat-endpoint round trip: the model's `read_file` call sent back with its result, the call carrying `signature`.
fn round_trip(signature: Option<&str>) -> Value {
    let mut call = json!({"id":"call-0","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"a\"}"}});
    if let Some(signature) = signature {
        let mut extra = json!(signature);
        for key in THOUGHT_SIGNATURE_PATH.iter().rev() {
            extra = json!({ *key: extra });
        }
        if let (Some(call), Some(extra)) = (call.as_object_mut(), extra.as_object()) {
            call.extend(extra.clone());
        }
    }
    let mut body = conforming(FakeProfile::GeminiOpenaiCompat);
    body["messages"] = json!([
        {"role":"system","content":"Be brief."},
        {"role":"user","content":"read a"},
        {"role":"assistant","content":null,"tool_calls":[call]},
        {"role":"tool","tool_call_id":"call-0","content":"contents"},
    ]);
    body
}

#[test]
fn tk_u_016_the_compatible_endpoint_requires_each_calls_signature_back() -> TestResult {
    let profile = FakeProfile::GeminiOpenaiCompat;
    // The signature it gave: accepted.
    let signed = round_trip(Some(&thought_signature("call-0")));
    assert_eq!(profile.check(profile.path(), &signed), None);
    // Missing or altered: refused with the provider's words, naming the call and the message's position.
    for body in [round_trip(None), round_trip(Some("not-the-signature"))] {
        let refusal = profile.check(profile.path(), &body).ok_or("accepted")?;
        assert_eq!((refusal.rule, refusal.status), ("thought_signature", 400));
        let error: Value = serde_json::from_str(&refusal.body)?;
        assert_eq!(error[0]["error"]["status"], "INVALID_ARGUMENT");
        assert_eq!(
            profile.error_message(&error).as_deref(),
            Some(
                "Function call is missing a thought_signature in functionCall parts. This is required for tools to work correctly, and missing thought_signature may lead to degraded model performance. Additional data, function call `default_api:read_file` , position 2."
            )
        );
    }
    Ok(())
}

#[tokio::test]
async fn tk_u_016_the_compatible_endpoint_signs_each_streamed_tool_call() -> TestResult {
    let profile = FakeProfile::GeminiOpenaiCompat;
    let script = Script {
        header_delay: std::time::Duration::ZERO,
        events: vec![Scripted::ToolCall {
            name: "read_file".into(),
            arguments_json: r#"{"path":"a"}"#.into(),
            split_into: 2,
        }],
    };
    let server = FakeModelServer::start_as(
        profile,
        vec![script],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    )
    .await?;
    let text = reqwest::Client::new()
        .post(format!("{}/chat/completions", server.base_url()))
        .json(&conforming(profile))
        .send()
        .await?
        .text()
        .await?;
    let calls: Vec<Value> = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .flat_map(|chunk| {
            chunk["choices"][0]["delta"]["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    let first = calls.first().ok_or("no tool call streamed")?;
    let id = first["id"]
        .as_str()
        .ok_or("the first piece carries the id")?;
    let mut signature = first;
    for key in THOUGHT_SIGNATURE_PATH {
        signature = &signature[key];
    }
    assert_eq!(
        signature.as_str(),
        Some(thought_signature(id).as_str()),
        "{first}"
    );
    assert!(
        calls[1..]
            .iter()
            .all(|piece| piece.get("extra_content").is_none()),
        "signed once, with the id"
    );
    Ok(())
}
