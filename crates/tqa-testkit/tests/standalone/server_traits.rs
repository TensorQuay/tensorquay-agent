//! Two model-server traits a scenario can ask for, so a stack can test the Models check's findings on them (c16):
//! - TK-U-046: `"ollama"` names models and the context each is loaded with; the server then answers Ollama's own
//!   `GET /api/version` and `GET /api/ps` (no key, as Ollama), which the route check reads. Without it, both are
//!   404, as before.
//! - TK-U-047: `"refuses_stream_options"` names models whose Chat Completions requests carrying `stream_options` get a
//!   400 naming it, as some OpenAI-compatible servers answer; the same request without `stream_options` gets the
//!   script, so the route check's retry without usage passes.
use super::support::*;
use serde_json::{Value, json};

fn scenario(extra: &Value) -> String {
    let mut document = json!({"model": "fixture", "responses": [
        {"events": [{"type": "text", "text": "Scripted."}], "usage": {"prompt_tokens": 10, "completion_tokens": 2}}
    ]});
    if let (Some(document), Some(extra)) = (document.as_object_mut(), extra.as_object()) {
        document.extend(extra.clone());
    }
    document.to_string()
}

/// A GET with no key, as the route check reads Ollama's own endpoints.
async fn get(server: &Binary, path: &str) -> Result<(u16, String), Box<dyn std::error::Error>> {
    let response = server
        .client
        .get(format!("{}{path}", server.base))
        .send()
        .await?;
    Ok((response.status().as_u16(), response.text().await?))
}

/// A streamed Chat Completions request for `model`, with or without `stream_options`.
async fn chat(
    server: &Binary,
    model: &str,
    usage: bool,
) -> Result<(u16, String), Box<dyn std::error::Error>> {
    let mut body =
        json!({"model": model, "stream": true, "messages": [{"role": "user", "content": "Hello"}]});
    if usage {
        body["stream_options"] = json!({"include_usage": true});
    }
    let response = server
        .client
        .post(format!("{}/v1/chat/completions", server.base))
        .bearer_auth(TOKEN)
        .json(&body)
        .send()
        .await?;
    Ok((response.status().as_u16(), response.text().await?))
}

#[tokio::test]
async fn tk_u_046_ollama_names_its_loaded_models_and_their_context() -> TestResult {
    let server = Binary::start(&scenario(
        &json!({"ollama": {"qwen3:4b": 4096, "big": 32768}}),
    ))
    .await?;
    let (status, version) = get(&server, "/api/version").await?;
    assert_eq!(status, 200, "{version}");
    let version: Value = serde_json::from_str(&version)?;
    assert!(
        version["version"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "{version}"
    );
    let (status, loaded) = get(&server, "/api/ps").await?;
    assert_eq!(status, 200, "{loaded}");
    let loaded: Value = serde_json::from_str(&loaded)?;
    let models = loaded["models"].as_array().ok_or("no models")?;
    assert_eq!(models.len(), 2, "{loaded}");
    let small = models
        .iter()
        .find(|model| model["name"] == "qwen3:4b")
        .ok_or("qwen3:4b not listed")?;
    assert_eq!(small["model"], "qwen3:4b");
    assert_eq!(small["context_length"], 4096);
    // The script still answers the named models.
    let (status, wire) = chat(&server, "qwen3:4b", false).await?;
    assert_eq!(status, 200, "{wire}");
    assert!(wire.contains("Scripted."), "{wire}");
    Ok(())
}

#[tokio::test]
async fn tk_u_046_without_ollama_its_endpoints_are_not_found() -> TestResult {
    let server = Binary::start(&scenario(&json!({}))).await?;
    for path in ["/api/version", "/api/ps"] {
        let (status, _) = get(&server, path).await?;
        assert!(matches!(status, 401 | 404), "{path}: {status}");
    }
    Ok(())
}

#[tokio::test]
async fn tk_u_046_an_unusable_ollama_entry_is_refused() -> TestResult {
    for ollama in [
        json!({"qwen3:4b": 0}),
        json!({"": 4096}),
        json!({"qwen3:4b": "4096"}),
        json!([]),
    ] {
        assert!(
            Binary::start(&scenario(&json!({"ollama": ollama})))
                .await
                .is_err(),
            "{ollama}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn tk_u_047_stream_options_are_refused_only_for_the_named_models() -> TestResult {
    let server = Binary::start(&scenario(&json!({"refuses_stream_options": ["no-usage"]}))).await?;
    // Refused with a 400 naming the field, as the route check recognises it.
    let (status, body) = chat(&server, "no-usage", true).await?;
    assert_eq!(status, 400, "{body}");
    assert!(body.contains("stream_options"), "{body}");
    // Without it, the same model gets the script, usage-less.
    let (status, wire) = chat(&server, "no-usage", false).await?;
    assert_eq!(status, 200, "{wire}");
    assert!(wire.contains("Scripted."), "{wire}");
    // Any other model takes stream_options as before.
    let (status, wire) = chat(&server, "fixture", true).await?;
    assert_eq!(status, 200, "{wire}");
    Ok(())
}

#[tokio::test]
async fn tk_u_047_an_unusable_refusal_list_is_refused() -> TestResult {
    for refuses in [json!([""]), json!("no-usage"), json!([1])] {
        assert!(
            Binary::start(&scenario(&json!({"refuses_stream_options": refuses})))
                .await
                .is_err(),
            "{refuses}"
        );
    }
    Ok(())
}
