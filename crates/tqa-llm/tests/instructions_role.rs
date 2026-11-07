//! LLM-U-038 and LLM-U-039: the `instructions_role` route setting, validated per protocol and applied by the relay's
//! one shaping pipeline. The runtime always sends the portable default; the relay rewrites only that role.
use proptest::prelude::*;
use serde_json::{Value, json};
use std::error::Error;
use tqa_llm::{Registry, Route, relay_shaping::shape_request};

type TestResult = Result<(), Box<dyn Error>>;

fn route(protocol: &str, setting: Option<&str>) -> Result<Route, Box<dyn Error>> {
    let setting = setting.map_or_else(String::new, |value| {
        format!("instructions_role='{value}'\n")
    });
    let registry = Registry::from_toml(&format!(
        "[[routes]]\nname='r'\nprotocol='{protocol}'\nbase_url='https://models.example/v1'\nmodel='m'\n\
         max_output=1000\n{setting}"
    ))?;
    Ok(registry.get("r").ok_or("missing route")?.clone())
}

#[test]
fn llm_u_038_each_protocol_takes_its_own_values_and_defaults() -> TestResult {
    for (protocol, allowed, refused) in [
        (
            "openai_chat",
            &["system", "developer"][..],
            &["top_level", "user", ""][..],
        ),
        (
            "openai_responses",
            &["top_level", "developer"][..],
            &["system", "user", ""][..],
        ),
        (
            "anthropic_messages",
            &["top_level"][..],
            &["system", "developer", ""][..],
        ),
    ] {
        route(protocol, None)?;
        for value in allowed {
            route(protocol, Some(value)).map_err(|error| format!("{protocol} {value}: {error}"))?;
        }
        for value in refused {
            let error = route(protocol, Some(value))
                .err()
                .ok_or_else(|| format!("{protocol} accepted {value:?}"))?
                .to_string();
            assert!(
                error.contains("instructions_role"),
                "{protocol} {value:?}: {error}"
            );
            for name in allowed {
                assert!(
                    error.contains(name),
                    "{protocol}: the error names {name}: {error}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn llm_u_038_a_stored_record_round_trips_the_setting() -> TestResult {
    for setting in [None, Some("developer")] {
        let route = route("openai_chat", setting)?;
        let config = tqa_llm::RouteConfig::from(route);
        let json = serde_json::to_value(&config)?;
        assert_eq!(
            json.get("instructions_role").and_then(Value::as_str),
            setting,
            "{json}"
        );
        let back: tqa_llm::RouteConfig = serde_json::from_value(json)?;
        assert_eq!(back, config);
    }
    Ok(())
}

fn chat_body() -> Value {
    json!({"model":"x","stream":true,"messages":[
        {"role":"system","content":"system prompt\n\ninstructions"},
        {"role":"user","content":"task"},
        {"role":"user","content":"[System notice] Environment changed."},
    ]})
}

fn responses_body() -> Value {
    json!({"model":"x","stream":true,"store":false,"instructions":"system prompt",
        "input":[{"type":"message","role":"user","content":"task"},
                 {"type":"message","role":"user","content":"[System notice] Environment changed."}]})
}

fn shaped(route: &Route, mut body: Value) -> Result<Value, Box<dyn Error>> {
    shape_request(route, &mut body)?;
    Ok(body)
}

#[test]
fn llm_u_039_chat_developer_changes_only_the_first_messages_role() -> TestResult {
    let default = shaped(&route("openai_chat", None)?, chat_body())?;
    let developer = shaped(&route("openai_chat", Some("developer"))?, chat_body())?;
    assert_eq!(developer["messages"][0]["role"], "developer");
    let mut expected = default.clone();
    expected["messages"][0]["role"] = json!("developer");
    assert_eq!(developer, expected, "nothing else changes");
    // A body whose first message isn't the system message is untouched.
    let mut no_system = chat_body();
    no_system["messages"]
        .as_array_mut()
        .ok_or("messages")?
        .remove(0);
    let route = route("openai_chat", Some("developer"))?;
    assert_eq!(
        shaped(&route, no_system.clone())?["messages"],
        no_system["messages"]
    );
    Ok(())
}

#[test]
fn llm_u_039_responses_developer_moves_the_instructions_to_one_leading_message() -> TestResult {
    let default = shaped(&route("openai_responses", None)?, responses_body())?;
    let developer = shaped(
        &route("openai_responses", Some("developer"))?,
        responses_body(),
    )?;
    assert!(developer.get("instructions").is_none(), "{developer}");
    assert_eq!(
        developer["input"][0],
        json!({"type":"message","role":"developer","content":"system prompt"})
    );
    assert_eq!(
        developer["input"].as_array().map(|input| &input[1..]),
        default["input"].as_array().map(Vec::as_slice)
    );
    // No instructions: nothing to move.
    let mut bare = responses_body();
    bare.as_object_mut().ok_or("body")?.remove("instructions");
    let route = route("openai_responses", Some("developer"))?;
    assert_eq!(shaped(&route, bare.clone())?["input"], bare["input"]);
    Ok(())
}

#[test]
fn llm_u_039_responses_developer_never_loses_the_instructions() -> TestResult {
    let route = route("openai_responses", Some("developer"))?;
    // A string input is one user message: it becomes that message, after the developer message.
    let string_input =
        json!({"model":"m","stream":true,"store":false,"instructions":"Be brief.","input":"hi"});
    let shaped_string = shaped(&route, string_input)?;
    assert!(
        shaped_string.get("instructions").is_none(),
        "{shaped_string}"
    );
    assert_eq!(
        shaped_string["input"],
        json!([{"type":"message","role":"developer","content":"Be brief."},
               {"type":"message","role":"user","content":"hi"}])
    );
    // No input, or an input of another shape: the instructions stay where they are.
    for input in [None, Some(json!(7))] {
        let mut body = json!({"model":"m","stream":true,"store":false,"instructions":"Be brief."});
        if let Some(input) = input {
            body["input"] = input;
        }
        let result = shaped(&route, body.clone())?;
        assert_eq!(result["instructions"], "Be brief.", "{result}");
        assert_eq!(result.get("input"), body.get("input"));
    }
    // Empty instructions stay as they are: no empty developer message.
    let mut empty = responses_body();
    empty["instructions"] = json!("");
    let result = shaped(&route, empty.clone())?;
    assert_eq!(
        (&result["instructions"], &result["input"]),
        (&json!(""), &empty["input"])
    );
    Ok(())
}

#[test]
fn llm_u_039_both_relay_settings_apply_together_in_one_pipeline() -> TestResult {
    // instructions_role and the tool-schema settings touch disjoint parts: tools stay as the tool step leaves them.
    let registry = Registry::from_toml(
        "[[routes]]\nname='r'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='m'\n\
         instructions_role='developer'\nstrict='off'\n",
    )?;
    let route = registry.get("r").ok_or("missing route")?.clone();
    let mut body = chat_body();
    body["tools"] = json!([{"type":"function","function":{"name":"read_file","description":"Read","strict":true,
        "parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],
            "additionalProperties":false}}}]);
    let body = shaped(&route, body)?;
    assert_eq!(body["messages"][0]["role"], "developer");
    assert!(
        body["tools"][0]["function"]
            .get("strict")
            .is_none_or(|strict| strict == false),
        "{body}"
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, failure_persistence: None, rng_seed: proptest::test_runner::RngSeed::Fixed(5039), ..ProptestConfig::default() })]
    #[test]
    fn llm_u_039_property_the_defaults_change_no_bytes(
        system in "[^\\p{C}]{0,40}", turns in proptest::collection::vec("[^\\p{C}]{0,20}", 0..6)
    ) {
        let fail = |error: Box<dyn Error>| TestCaseError::fail(error.to_string());
        let mut messages = vec![json!({"role":"system","content":system})];
        messages.extend(turns.iter().map(|turn| json!({"role":"user","content":turn})));
        let chat = json!({"model":"m","stream":true,"messages":messages});
        let mut input: Vec<Value> = turns.iter().map(|turn| json!({"type":"message","role":"user","content":turn})).collect();
        input.push(json!({"type":"message","role":"user","content":"last"}));
        let responses = json!({"model":"m","stream":true,"store":false,"instructions":system,"input":input});
        // Defaults: the relay's result equals the result with no setting at all, byte for byte.
        for (protocol, body, explicit) in [("openai_chat", &chat, "system"), ("openai_responses", &responses, "top_level")] {
            let implicit = shaped(&route(protocol, None).map_err(fail)?, body.clone()).map_err(fail)?;
            let stated = shaped(&route(protocol, Some(explicit)).map_err(fail)?, body.clone()).map_err(fail)?;
            prop_assert_eq!(serde_json::to_vec(&implicit)?, serde_json::to_vec(&stated)?);
        }
    }
}
