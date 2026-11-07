//! LLM-U-037: the leading system messages go as the top-level `instructions`, never as `system` or `developer` input
//! messages, which some servers refuse ("System messages are not allowed"; a template's "Unexpected message role.").

use super::request;
use crate::{CompletionRequest, ContentPart, Message, Registry, Role, Route};
use proptest::prelude::*;
use serde_json::{Value, json};
use std::error::Error;

const NOTICE: &str = "[System notice] ";

fn route() -> Result<Route, Box<dyn Error>> {
    Registry::from_toml("[[routes]]\nname='test'\nprotocol='openai_responses'\nbase_url='https://models.example/v1/'\nmodel='test-model'\nmax_output=100")?.get("test").cloned().ok_or_else(|| "missing test route".into())
}
fn message(role: Role, text: &str) -> Message {
    Message {
        role,
        content: vec![ContentPart::Text(text.into())],
        tool_calls: vec![],
        tool_call_id: None,
        name: None,
    }
}
fn encoded(messages: Vec<Message>) -> Result<Value, Box<dyn Error>> {
    let request = CompletionRequest {
        messages,
        ..CompletionRequest::default()
    };
    Ok(serde_json::from_slice(&request::encode(
        &route()?,
        &request,
    )?)?)
}
fn roles(body: &Value) -> Vec<String> {
    body["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["role"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn llm_u_037_the_captured_request_sends_its_instructions_in_the_top_level_field()
-> Result<(), Box<dyn Error>> {
    // The captured shape: system prompt, an empty instructions slot, the tool catalogue, then four user turns.
    let body = encoded(vec![
        message(Role::System, "You are a coding agent."),
        message(Role::System, ""),
        message(Role::System, r#"[{"name":"edit_file"}]"#),
        message(Role::User, "hello"),
        message(Role::User, "hello"),
        message(Role::User, "try again "),
        message(Role::User, "hi"),
    ])?;
    assert_eq!(
        body["instructions"],
        json!("You are a coding agent.\n\n[{\"name\":\"edit_file\"}]")
    );
    assert_eq!(
        body["input"],
        json!([
            {"type":"message","role":"user","content":"hello"},
            {"type":"message","role":"user","content":"hello"},
            {"type":"message","role":"user","content":"try again "},
            {"type":"message","role":"user","content":"hi"},
        ])
    );
    Ok(())
}

#[test]
fn llm_u_037_a_later_notice_goes_as_user_text_and_no_system_role_is_sent()
-> Result<(), Box<dyn Error>> {
    let body = encoded(vec![
        message(Role::System, "system prompt"),
        message(Role::System, "instructions"),
        message(Role::User, "task"),
        message(Role::Assistant, "done"),
        message(Role::System, "Environment changed to workstation."),
        message(Role::User, "more"),
    ])?;
    assert_eq!(body["instructions"], json!("system prompt\n\ninstructions"));
    assert_eq!(
        body["input"][2],
        json!({"type":"message","role":"user","content":format!("{NOTICE}Environment changed to workstation.")})
    );
    assert_eq!(roles(&body), ["user", "assistant", "user", "user"]);
    Ok(())
}

#[test]
fn llm_u_037_a_single_empty_absent_or_mixed_system_message() -> Result<(), Box<dyn Error>> {
    let user = || message(Role::User, "u");
    let only_user = json!([{"type":"message","role":"user","content":"u"}]);
    // A single one is the instructions.
    let body = encoded(vec![message(Role::System, "only"), user()])?;
    assert_eq!(
        (&body["instructions"], &body["input"]),
        (&json!("only"), &only_user)
    );
    // Empty or absent: no instructions field at all, never an empty one.
    for messages in [
        vec![message(Role::System, ""), user()],
        vec![message(Role::System, ""), message(Role::System, ""), user()],
        vec![user()],
    ] {
        let body = encoded(messages)?;
        assert!(body.get("instructions").is_none(), "{body}");
        assert_eq!(body["input"], only_user);
    }
    // Only text parts are merged.
    let mixed = Message {
        content: vec![
            ContentPart::Text("before ".into()),
            ContentPart::Image {
                media_type: tqa_types::ImageMediaType::Png,
                data: vec![0x89, b'P', b'N', b'G'],
            },
            ContentPart::Text("after".into()),
        ],
        ..message(Role::System, "")
    };
    let body = encoded(vec![mixed, user()])?;
    assert_eq!(body["instructions"], json!("before after"));
    Ok(())
}

#[test]
fn llm_u_037_extra_body_cannot_set_the_instructions() {
    let route = Registry::from_toml(
        "[[routes]]\nname='test'\nprotocol='openai_responses'\nbase_url='https://models.example/v1/'\nmodel='test-model'\nmax_output=100\nextra_body={instructions='override'}",
    );
    assert!(route.is_err(), "instructions stays harness-owned");
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, rng_seed: proptest::test_runner::RngSeed::Fixed(5037), ..ProptestConfig::default() })]
    #[test]
    fn llm_u_037_property_the_instructions_stay_byte_identical_across_steps(
        system in "[^\\p{C}]{1,40}", instructions in "[^\\p{C}]{0,40}", tools in "[^\\p{C}]{1,40}",
        turns in proptest::collection::vec("[^\\p{C}]{0,20}", 20)
    ) {
        let fail = |error: Box<dyn Error>| TestCaseError::fail(error.to_string());
        let slots = [
            message(Role::System, &system),
            message(Role::System, &instructions),
            message(Role::System, &tools),
        ];
        let merged: Vec<&str> = [system.as_str(), instructions.as_str(), tools.as_str()]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect();
        let expected = serde_json::to_string(&merged.join("\n\n"))?;
        let mut history = Vec::new();
        for turn in turns {
            history.push(message(Role::User, &turn));
            history.push(message(Role::System, &format!("notice {turn}")));
            let body = encoded(slots.iter().cloned().chain(history.iter().cloned()).collect()).map_err(fail)?;
            prop_assert_eq!(serde_json::to_string(&body["instructions"])?, expected.clone());
            prop_assert!(roles(&body).iter().all(|role| role == "user"));
        }
    }
}
