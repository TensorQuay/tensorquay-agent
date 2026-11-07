//! TK-U-037 (the gateway's side): the real route check, through the relay's own shaping and decoder, against the
//! standalone fake model that stacks use (`tqa-fake-model`). The fake answers the check's fixed conversation itself,
//! with the check's own nonce, whatever its scenario: here one whose only reply belongs to another conversation under
//! prompt selection, so any request that reached the scenario would be refused with 422.

use super::RelayConfig;
use super::probe::{Check, Observation, Probe, run};
use super::probe_tests::{NONCE, TestResult, limiter};
use std::io::Write as _;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tqa_testkit::standalone::{Scenario, serve};

const SCENARIO: &str = r#"{"model": "fixture", "select": "prompt", "responses": [
    {"when": "Some task's own first message", "events": [{"type": "text", "text": "Not the check's."}],
     "usage": {"prompt_tokens": 1, "completion_tokens": 1}}]}"#;

/// A route to the fake with a bearer key, which the fake requires (any non-empty token).
fn route(
    base: &str,
    protocol: &str,
    extra: &str,
) -> Result<tqa_llm::Route, Box<dyn std::error::Error>> {
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='fake'\nprotocol='{protocol}'\nbase_url='{base}'\nmodel='fixture'\n\
         auth={{kind='bearer'}}\nkey_ref='secrets/local'\nmax_output=4096\nstream_usage=true\n{extra}"
    ))?;
    Ok(registry.get("fake").ok_or("route")?.clone())
}

async fn probe(route: &tqa_llm::Route) -> Result<Observation, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()?;
    let config = RelayConfig::default();
    let probe = Probe {
        client: &client,
        config: &config,
        limiter: limiter(route),
        secret: Some(secrecy::SecretString::from("synthetic-fake-model-key")),
        nonce: NONCE.into(),
    };
    Ok(run(route, probe, &CancellationToken::new()).await)
}

async fn start() -> Result<(String, CancellationToken), Box<dyn std::error::Error>> {
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(SCENARIO.as_bytes())?;
    let scenario = Scenario::load(file.path())?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}/v1", listener.local_addr()?);
    let cancel = CancellationToken::new();
    tokio::spawn(serve(listener, scenario, cancel.clone()));
    Ok((base, cancel))
}

#[tokio::test]
async fn tk_u_037_the_fake_model_passes_the_route_check_on_every_protocol() -> TestResult {
    let (base, cancel) = start().await?;
    for (protocol, extra) in [
        ("openai_chat", ""),
        ("openai_responses", ""),
        ("anthropic_messages", ""),
        // Without native tools the check asks for text only.
        ("openai_chat", "native_tools=false\n"),
    ] {
        let route = route(&base, protocol, extra)?;
        let observed = probe(&route).await?;
        assert!(observed.ok, "{protocol} {extra}: {observed:?}");
        assert!(
            observed.failure.is_none() && observed.warnings.is_empty(),
            "{protocol} {extra}: {observed:?}"
        );
        if extra.is_empty() {
            assert_eq!(
                (observed.tool_call, observed.round_trip),
                (Check::Passed, Check::Passed),
                "{protocol}"
            );
        }
        // Chat and Responses send strict tools, so they also make the strict-string check; its line break is kept.
        if extra.is_empty() && protocol != "anthropic_messages" {
            assert_eq!(observed.strict_line_breaks, Check::Passed, "{protocol}");
        }
    }
    cancel.cancel();
    Ok(())
}
