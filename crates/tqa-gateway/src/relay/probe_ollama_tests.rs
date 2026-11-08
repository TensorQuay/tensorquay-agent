//! GW-U-153 (a first-hour check): on a local route whose server is Ollama, the check reads the context the loaded
//! model runs with. Below 16,384 it fails, suggesting the route's Context window be set to the server's number; above,
//! a route whose Context window says otherwise gets a warning; any other server is as before.
use super::probe_tests::{
    NONCE, TestResult, call, good, limiter, lines, probe, route, start_model,
};
use tqa_testkit::{Script, Scripted};

// TEST: GW-U-153
#[tokio::test]
async fn gw_u_153_a_small_ollama_context_fails_the_check_with_the_servers_number() -> TestResult {
    let server = start_model(good()).await?;
    server.set_ollama(Some(vec![("m:latest".into(), 4096)]));
    let probed = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&probed, limiter(&probed)).await?;
    assert!(!observed.ok, "{observed:?}");
    let failure = observed.failure.as_ref().ok_or("no failure")?;
    assert_eq!((failure.code, failure.status), ("context_too_small", None));
    let suggestion = failure.suggestion.as_ref().ok_or("no suggestion")?;
    assert_eq!(
        (suggestion.field, suggestion.value.as_ref()),
        ("context_window", "4096"),
        "one click sets what the server has, so tasks compact in time"
    );
    let context = observed.server_context.as_ref().ok_or("no context")?;
    assert_eq!(
        (context.server, context.context_length, context.minimum),
        ("ollama", 4096, 16_384),
        "both numbers, for the words"
    );
    let shown = serde_json::to_value(&observed)?;
    assert_eq!(
        shown["server_context"],
        serde_json::json!({"server": "ollama", "context_length": 4096, "minimum": 16384})
    );
    assert!(observed.warnings.is_empty(), "{observed:?}");
    server.shutdown().await?;
    Ok(())
}

// TEST: GW-U-153
#[tokio::test]
async fn gw_u_153_enough_context_passes_and_warns_only_when_the_route_says_otherwise() -> TestResult
{
    for (extra, warned) in [
        ("", true),
        ("context_window=8192\n", true),
        ("context_window=65536\n", true),
        ("context_window=32768\n", false),
    ] {
        let server = start_model(good()).await?;
        server.set_ollama(Some(vec![("m".into(), 32_768)]));
        let probed = route("openai_chat", server.base_url(), extra)?;
        let observed = probe(&probed, limiter(&probed)).await?;
        assert!(observed.ok, "{extra}: {observed:?}");
        assert_eq!(
            observed.warnings.contains(&"context_window_differs"),
            warned,
            "{extra}: {observed:?}"
        );
        assert_eq!(
            observed
                .server_context
                .as_ref()
                .map(|context| context.context_length),
            Some(32_768)
        );
        server.shutdown().await?;
    }
    Ok(())
}

// TEST: GW-U-153
#[tokio::test]
async fn gw_u_153_another_server_or_an_unlisted_model_changes_nothing() -> TestResult {
    for ollama in [None, Some(vec![("other:latest".to_owned(), 4096)])] {
        let server = start_model(good()).await?;
        server.set_ollama(ollama.clone());
        let probed = route("openai_chat", server.base_url(), "")?;
        let observed = probe(&probed, limiter(&probed)).await?;
        assert!(
            observed.ok && observed.warnings.is_empty(),
            "{ollama:?}: {observed:?}"
        );
        assert!(observed.server_context.is_none(), "{ollama:?}");
        assert!(
            serde_json::to_value(&observed)?
                .get("server_context")
                .is_none()
        );
        server.shutdown().await?;
    }
    Ok(())
}

// TEST: GW-U-153
#[tokio::test]
async fn gw_u_153_a_small_context_wins_over_a_usage_refusal_and_a_failed_check_asks_nothing()
-> TestResult {
    let refused = Scripted::Status {
        code: 400,
        body: r#"{"error":{"message":"Unrecognized request argument supplied: stream_options"}}"#
            .into(),
        retry_after: None,
    };
    let server = start_model(vec![
        Script::new(vec![refused]),
        Script::new(vec![call(NONCE)]),
        Script::new(vec![Scripted::Text("ok".into())]),
        Script::new(vec![lines(&format!("{NONCE}\n{NONCE}"))]),
    ])
    .await?;
    server.set_ollama(Some(vec![("m".into(), 2048)]));
    let probed = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&probed, limiter(&probed)).await?;
    let failure = observed.failure.as_ref().ok_or("no failure")?;
    assert_eq!(failure.code, "context_too_small", "{observed:?}");
    server.shutdown().await?;
    // A check that failed on its own never asks: its model may not be loaded, and its failure says enough.
    let server = start_model(vec![]).await?;
    server.set_ollama(Some(vec![("m".into(), 2048)]));
    let probed = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&probed, limiter(&probed)).await?;
    assert!(observed.server_context.is_none(), "{observed:?}");
    assert_ne!(
        observed.failure.as_ref().map(|failure| failure.code),
        Some("context_too_small")
    );
    server.shutdown().await?;
    Ok(())
}
