//! TK-U-018 (the dry mode's endpoint): `--profile` makes the standalone binary answer as a hosted provider's fake does.
//! A request that breaks one of the profile's rules gets that provider's refusal, byte for byte as the in-process fake
//! gives it; a conforming request is served from the scenario as before; an unknown profile stops the binary.
use super::support::*;
use serde_json::{Value, json};
use tqa_testkit::FakeProfile;

/// A Chat request with one strict tool whose schema is open, which the strict Chat profile refuses.
fn open_strict_tool() -> Value {
    let mut body = request(0);
    body["tools"] = json!([{"type":"function","function":{"name":"read_file","description":"Read","strict":true,
        "parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}]);
    body
}

async fn post(
    server: &Binary,
    path: &str,
    body: &Value,
) -> Result<(u16, String), Box<dyn std::error::Error>> {
    let response = server
        .client
        .post(format!("{}{path}", server.base))
        .bearer_auth(TOKEN)
        .json(body)
        .send()
        .await?;
    Ok((response.status().as_u16(), response.text().await?))
}

#[tokio::test]
async fn tk_u_018_a_profile_refuses_as_the_in_process_fake_does() -> TestResult {
    let server = Binary::start_with(DEMO, &["--profile", "chat-strict"]).await?;
    let body = open_strict_tool();
    let expected = FakeProfile::DeepseekChat
        .check("/v1/chat/completions", &body)
        .ok_or("the in-process fake serves it")?;
    assert_eq!(
        post(&server, "/v1/chat/completions", &body).await?,
        (expected.status, expected.body)
    );
    // A conforming request is served from the scenario.
    let (status, wire) = post(&server, "/v1/chat/completions", &request(0)).await?;
    assert_eq!(status, 200, "{wire}");
    assert!(wire.ends_with("data: [DONE]\n\n"));
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn tk_u_018_each_profile_has_a_flag_and_serves_only_its_path() -> TestResult {
    for (flag, profile) in [
        ("chat-strict", FakeProfile::DeepseekChat),
        ("chat-compat", FakeProfile::GeminiOpenaiCompat),
        ("responses", FakeProfile::OpenaiResponses),
        ("responses-backend", FakeProfile::OpenaiResponsesBackend),
        ("messages", FakeProfile::AnthropicMessages),
    ] {
        let server = Binary::start_with(DEMO, &["--profile", flag]).await?;
        let other = if profile.path() == "/v1/messages" {
            "/v1/chat/completions"
        } else {
            "/v1/messages"
        };
        let expected = profile
            .check(other, &request(0))
            .ok_or("another path is served")?;
        assert_eq!(
            post(&server, other, &request(0)).await?,
            (expected.status, expected.body),
            "{flag}"
        );
        server.stop().await?;
    }
    Ok(())
}

#[tokio::test]
async fn tk_u_018_an_unknown_profile_stops_the_binary() -> TestResult {
    for flags in [
        &["--profile", "unknown"][..],
        &["--profile", "chat-strict", "--profile", "messages"],
    ] {
        assert!(Binary::start_with(DEMO, flags).await.is_err(), "{flags:?}");
    }
    Ok(())
}

/// The compatible Chat profile streams a tool call as that endpoint does, byte for byte as the in-process fake restyles
/// it: deltas without `index`, and each call signed on the piece that carries its id. The strict Chat profile streams
/// the scenario unchanged.
#[tokio::test]
async fn tk_u_018_the_compatible_chat_profile_streams_calls_restyled() -> TestResult {
    let plain = Binary::start(DEMO).await?;
    let (status, unstyled) = post(&plain, "/v1/chat/completions", &request(0)).await?;
    plain.stop().await?;
    assert_eq!(status, 200, "{unstyled}");
    assert!(
        unstyled.contains("\"index\":"),
        "the scenario's calls carry an index"
    );

    let compat = Binary::start_with(DEMO, &["--profile", "chat-compat"]).await?;
    assert!(
        FakeProfile::GeminiOpenaiCompat
            .check("/v1/chat/completions", &request(0))
            .is_none(),
        "the request conforms"
    );
    let (status, restyled) = post(&compat, "/v1/chat/completions", &request(0)).await?;
    compat.stop().await?;
    assert_eq!(status, 200, "{restyled}");
    let expected = FakeProfile::GeminiOpenaiCompat.restyle_chat_frame(unstyled.as_bytes());
    assert_eq!(restyled.as_bytes(), expected.as_slice());
    let calls: Vec<Value> = chunks(&restyled)?
        .iter()
        .filter_map(|chunk| {
            chunk["choices"][0]["delta"]["tool_calls"]
                .as_array()
                .cloned()
        })
        .flatten()
        .collect();
    assert!(!calls.is_empty());
    assert!(
        calls.iter().all(|call| call.get("index").is_none()),
        "{calls:?}"
    );
    for call in calls.iter().filter(|call| call.get("id").is_some()) {
        let id = call["id"].as_str().ok_or("id")?;
        let signature = &call[tqa_testkit::THOUGHT_SIGNATURE_PATH[0]]
            [tqa_testkit::THOUGHT_SIGNATURE_PATH[1]][tqa_testkit::THOUGHT_SIGNATURE_PATH[2]];
        assert_eq!(signature, &json!(tqa_testkit::thought_signature(id)));
    }

    let strict = Binary::start_with(DEMO, &["--profile", "chat-strict"]).await?;
    let (_, strict_wire) = post(&strict, "/v1/chat/completions", &request(0)).await?;
    strict.stop().await?;
    assert_eq!(
        strict_wire, unstyled,
        "the strict profile streams the scenario unchanged"
    );
    Ok(())
}
