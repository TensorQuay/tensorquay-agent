//! GW-I-037 (engine): the route probe against the scripted fake model, through the relay's own shaping, credential,
//! limiter and decoder. The HTTP API and storage are tested separately (GW-I-037).

use super::{
    RelayConfig,
    probe::{Check, Probe, run},
};
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_llm::{Limits, QueueBounds, Route, RouteLimiter, SystemClock, TokioSleeper, Usage};
use tqa_testkit::{FakeModelServer, Script, Scripted};

pub(super) type TestResult = Result<(), Box<dyn std::error::Error>>;
pub(super) const NONCE: &str = "n0nce123";

/// A route with native tools sends strict tools unless `extra` says otherwise, so a probe that passes its round trip
/// makes a third request, the strict-string check (GW-U-064): script its answer ([`good`]) or set `strict='off'`.
pub(super) fn route(
    protocol: &str,
    base_url: &str,
    extra: &str,
) -> Result<Route, Box<dyn std::error::Error>> {
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='probed'\nprotocol='{protocol}'\nbase_url='{base_url}'\nmodel='m'\nauth={{kind='none'}}\n\
         max_output=4096\nstream_usage=true\n{extra}"
    ))?;
    Ok(registry.get("probed").ok_or("route")?.clone())
}

pub(super) fn limiter(route: &Route) -> Arc<RouteLimiter> {
    let bounds = QueueBounds {
        wait: Duration::from_secs(1),
        max_waiting: 4,
    };
    Arc::new(RouteLimiter::new(
        route.limits,
        bounds,
        Arc::new(SystemClock),
        Arc::new(TokioSleeper),
    ))
}

pub(super) async fn probe(
    route: &Route,
    limiter: Arc<RouteLimiter>,
) -> Result<super::probe::Observation, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()?;
    let config = RelayConfig::default();
    let probe = Probe {
        client: &client,
        config: &config,
        limiter,
        secret: None,
        nonce: NONCE.into(),
    };
    Ok(run(route, probe, &CancellationToken::new()).await)
}

pub(super) fn usage() -> Scripted {
    Scripted::Usage(Usage {
        prompt_tokens: 20,
        completion_tokens: 5,
        cached_tokens: Some(2),
        reasoning_tokens: None,
    })
}
pub(super) fn call(value: &str) -> Scripted {
    Scripted::ToolCall {
        name: "probe_echo".into(),
        arguments_json: serde_json::json!({ "value": value }).to_string(),
        split_into: 2,
    }
}
/// A request's expected output cap: the probe's `cap`, or for the strict-string check's request its own allowance of
/// 256 when that is larger (GW-U-064).
fn expected_cap(body: &serde_json::Value, cap: u64) -> u64 {
    if body.to_string().contains("probe_lines") {
        cap.max(256)
    } else {
        cap
    }
}

/// The strict-string check's call (GW-U-064).
pub(super) fn lines(value: &str) -> Scripted {
    Scripted::ToolCall {
        name: "probe_lines".into(),
        arguments_json: serde_json::json!({ "value": value }).to_string(),
        split_into: 2,
    }
}
/// A model that calls the probe tool with the nonce, then answers; on a route that sends strict tools, it then keeps
/// the two-line value's line break (GW-U-064).
pub(super) fn good() -> Vec<Script> {
    vec![
        Script::new(vec![call(NONCE), usage()]),
        Script::new(vec![Scripted::Text("ok".into()), usage()]),
        Script::new(vec![lines(&format!("{NONCE}\n{NONCE}")), usage()]),
    ]
}
pub(super) async fn start_model(
    scripts: Vec<Script>,
) -> Result<FakeModelServer, Box<dyn std::error::Error>> {
    Ok(FakeModelServer::start(scripts, Arc::new(TokioSleeper)).await?)
}

#[tokio::test]
async fn gw_i_037_a_good_route_is_verified_on_every_protocol() -> TestResult {
    for protocol in ["openai_chat", "openai_responses", "anthropic_messages"] {
        let server = start_model(good()).await?;
        let route = route(protocol, server.base_url(), "")?;
        let observed = probe(&route, limiter(&route)).await?;
        assert!(observed.ok, "{protocol}: {observed:?}");
        assert_eq!(
            (observed.tool_call, observed.round_trip),
            (Check::Passed, Check::Passed),
            "{protocol}"
        );
        assert_eq!(observed.streaming, Some(true));
        assert!(
            observed.usage.reported && observed.usage.cached,
            "{protocol}"
        );
        // Messages never sends strict tools, so it has no strict-string check (GW-U-064).
        let requests = if protocol == "anthropic_messages" {
            2
        } else {
            3
        };
        assert_eq!(
            (observed.tokens.prompt, observed.tokens.completion),
            (20 * requests, 5 * requests),
            "{protocol}"
        );
        assert!(
            observed.failure.is_none() && observed.warnings.is_empty(),
            "{protocol}: {observed:?}"
        );
        assert_eq!(
            server.requests().len(),
            usize::try_from(requests)?,
            "one call, its round trip and the strict-string check"
        );
        // The request was shaped like real traffic: the route's model, streaming, the probe tool.
        let first = &server.requests()[0].body;
        assert_eq!(first["model"], "m", "{protocol}");
        assert_eq!(first["stream"], true, "{protocol}");
        assert!(first.to_string().contains("probe_echo") && first.to_string().contains(NONCE));
        server.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn gw_i_037_failures_are_named() -> TestResult {
    // A wrong base URL path: the server answers 404, and names no model: no endpoint for this protocol there
    // (GW-U-154).
    let server = start_model(vec![]).await?;
    let wrong = route("openai_chat", &format!("{}/nowhere", server.base_url()), "")?;
    let observed = probe(&wrong, limiter(&wrong)).await?;
    assert_eq!(
        observed.failure.as_ref().map(|failure| failure.code),
        Some("endpoint_missing")
    );
    assert!(!observed.ok);
    server.shutdown().await?;
    // Nothing listens: unreachable.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")?;
    let base = format!("http://{}/v1", closed.local_addr()?);
    drop(closed);
    let unreachable = route("openai_chat", &base, "")?;
    let observed = probe(&unreachable, limiter(&unreachable)).await?;
    assert_eq!(
        observed.failure.as_ref().map(|failure| failure.code),
        Some("unreachable")
    );
    // At a loopback address the check also says whether the container host's name resolves here (GW-U-154).
    assert!(
        observed
            .failure
            .as_ref()
            .is_some_and(|failure| failure.container_host.is_some())
    );
    // A rejected credential.
    let status = Scripted::Status {
        code: 401,
        body: "no".into(),
        retry_after: None,
    };
    let server = start_model(vec![Script::new(vec![status])]).await?;
    let rejected = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&rejected, limiter(&rejected)).await?;
    assert_eq!(
        observed
            .failure
            .as_ref()
            .map(|failure| (failure.code, failure.status)),
        Some(("auth_rejected", Some(401)))
    );
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn gw_i_037_a_route_without_tool_support_and_one_without_usage() -> TestResult {
    // The model answers in text although the route declares native tools.
    let server = start_model(vec![Script::new(vec![
        Scripted::Text("I cannot call tools.".into()),
        usage(),
    ])])
    .await?;
    let text_only = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&text_only, limiter(&text_only)).await?;
    assert_eq!(
        observed.failure.as_ref().map(|failure| failure.code),
        Some("no_tool_call")
    );
    assert_eq!(
        (observed.tool_call, observed.round_trip),
        (Check::Failed, Check::NotRun)
    );
    server.shutdown().await?;
    // The route works but reports no usage: verified, with a warning.
    let quiet = vec![
        Script::new(vec![call(NONCE)]),
        Script::new(vec![Scripted::Text("ok".into())]),
        Script::new(vec![lines(&format!("{NONCE}\n{NONCE}"))]),
    ];
    let server = start_model(quiet).await?;
    let no_usage = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&no_usage, limiter(&no_usage)).await?;
    assert!(observed.ok, "{observed:?}");
    assert!(!observed.usage.reported);
    assert_eq!(observed.warnings, ["usage_missing"]);
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn gw_i_037_a_busy_route_is_reported_without_contacting_the_model() -> TestResult {
    let server = start_model(good()).await?;
    let busy = route("openai_chat", server.base_url(), "concurrency=1\n")?;
    let limiter = Arc::new(RouteLimiter::new(
        Limits {
            concurrency: Some(1),
            ..Limits::default()
        },
        QueueBounds {
            wait: Duration::from_millis(50),
            max_waiting: 1,
        },
        Arc::new(SystemClock),
        Arc::new(TokioSleeper),
    ));
    let held = limiter.acquire(&CancellationToken::new()).await?;
    let observed = probe(&busy, limiter.clone()).await?;
    assert_eq!(
        observed.failure.as_ref().map(|failure| failure.code),
        Some("rate_limited")
    );
    assert!(server.requests().is_empty());
    drop(held);
    server.shutdown().await?;
    Ok(())
}

/// GW-U-061: a stream the decoder refuses keeps the decoder's (redacted) words.
#[tokio::test]
async fn gw_u_061_a_protocol_error_keeps_the_decoders_words() -> TestResult {
    // A second completion choice, which the Chat decoder refuses.
    let piece = r#"{"choices":[{"index":1,"delta":{"content":"a"}}]}"#;
    let server = start_model(vec![Script::new(vec![Scripted::RawJson(piece.into())])]).await?;
    let route = route("openai_chat", server.base_url(), "")?;
    let observed = probe(&route, limiter(&route)).await?;
    let failure = observed.failure.as_ref().ok_or("no failure")?;
    assert_eq!(failure.code, "protocol_error");
    let detail = failure.detail.as_deref().ok_or("no detail")?;
    assert!(detail.contains("single completion choice"), "{detail}");
    server.shutdown().await?;
    Ok(())
}

/// GW-U-061: an answer that does not stream keeps its content type and the start of its body, bounded.
#[tokio::test]
async fn gw_u_061_a_non_streaming_answer_keeps_its_type_and_body_start() -> TestResult {
    let body = format!(
        "{{\"object\":\"response\",\"output\":[],\"pad\":\"{}\"}}",
        "x".repeat(2000)
    );
    let server = start_model(vec![Script::new(vec![
        Scripted::Status {
            code: 200,
            body: body.clone(),
            retry_after: None,
        },
        Scripted::Header("content-type".into(), "application/json".into()),
    ])])
    .await?;
    let route = route("openai_responses", server.base_url(), "")?;
    let observed = probe(&route, limiter(&route)).await?;
    let failure = observed.failure.as_ref().ok_or("no failure")?;
    assert_eq!((failure.code, failure.status), ("not_streaming", Some(200)));
    let detail = failure.detail.as_deref().ok_or("no detail")?;
    assert!(
        detail.starts_with("content-type: application/json; body: {\"object\":\"response\""),
        "{detail}"
    );
    assert!(detail.len() <= 600, "bounded: {}", detail.len());
    assert_eq!(observed.streaming, Some(false));
    server.shutdown().await?;
    Ok(())
}

/// GW-U-062: a stream labelled text/plain passes, with a warning; a text/plain answer that isn't a stream still fails.
#[tokio::test]
async fn gw_u_062_a_mislabelled_stream_passes_with_a_warning() -> TestResult {
    for protocol in ["openai_chat", "openai_responses"] {
        let relabel =
            || Scripted::Header("content-type".into(), "text/plain; charset=utf-8".into());
        let scripts = good()
            .into_iter()
            .map(|mut script| {
                script.events.push(relabel());
                script
            })
            .collect();
        let server = start_model(scripts).await?;
        let route = route(protocol, server.base_url(), "")?;
        let observed = probe(&route, limiter(&route)).await?;
        assert!(observed.ok, "{protocol}: {observed:?}");
        assert_eq!(observed.streaming, Some(true));
        assert_eq!(observed.warnings, vec!["stream_mislabelled"], "{protocol}");
        server.shutdown().await?;
    }
    let server = start_model(vec![Script::new(vec![
        Scripted::Status {
            code: 200,
            body: "{\"output\":[]}".into(),
            retry_after: None,
        },
        Scripted::Header("content-type".into(), "text/plain".into()),
    ])])
    .await?;
    let route = route("openai_responses", server.base_url(), "")?;
    let observed = probe(&route, limiter(&route)).await?;
    assert_eq!(
        observed.failure.as_ref().map(|failure| failure.code),
        Some("not_streaming")
    );
    server.shutdown().await?;
    Ok(())
}

/// The probe's output cap reaches the endpoint on every protocol, under the protocol's own field.
#[tokio::test]
async fn gw_i_037_the_probe_caps_its_output_on_every_protocol() -> TestResult {
    for (protocol, field) in [
        ("openai_chat", "max_tokens"),
        ("openai_responses", "max_output_tokens"),
        ("anthropic_messages", "max_tokens"),
    ] {
        let server = start_model(good()).await?;
        let route = route(protocol, server.base_url(), "")?;
        let observed = probe(&route, limiter(&route)).await?;
        assert!(observed.ok, "{protocol}: {observed:?}");
        for request in server.requests() {
            // The strict-string check has its own allowance (GW-U-064).
            assert_eq!(
                request.body[field],
                expected_cap(&request.body, 256),
                "{protocol}: {}",
                request.body
            );
        }
        server.shutdown().await?;
    }
    Ok(())
}

/// A route that declares reasoning gets room to think: min(its `max_output`, 4096); without it, 4096. A route
/// without reasoning gets 256, within its own `max_output` (a model that reasons unasked still reaches its call).
/// Never uncapped.
#[tokio::test]
async fn gw_i_037_a_reasoning_route_gets_a_bounded_thinking_allowance() -> TestResult {
    let effort =
        "reasoning={field='reasoning_effort',allowed=['minimal','low'],default='minimal'}\n";
    // (the route's extra lines, the probe's cap, the route's own limit)
    for (extra, cap, limit) in [
        ("", 256, u64::MAX),
        ("max_output=100\n", 100, 100),
        (effort, 4096, u64::MAX),
        (&format!("{effort}max_output=512\n") as &str, 512, 512),
        (
            &format!("{effort}max_output=100000\n") as &str,
            4096,
            100_000,
        ),
    ] {
        let server = start_model(good()).await?;
        let registry = tqa_llm::Registry::from_toml(&format!(
            "[[routes]]\nname='probed'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='m'\nauth={{kind='none'}}\n\
             stream_usage=true\n{extra}",
            server.base_url()
        ))?;
        let route = registry.get("probed").ok_or("route")?.clone();
        let observed = probe(&route, limiter(&route)).await?;
        assert!(observed.ok, "{extra}: {observed:?}");
        let effort = extra.contains("reasoning").then_some("minimal");
        for request in server.requests() {
            assert_eq!(
                request.body["max_tokens"],
                expected_cap(&request.body, cap).min(limit),
                "{extra}: {}",
                request.body
            );
            assert_eq!(request.body["reasoning_effort"].as_str(), effort, "{extra}");
        }
        server.shutdown().await?;
    }
    Ok(())
}

/// ADR-0027 (Chat tool-call extras): the round trip sends the first call's opaque extra back on that call, and the
/// observation names the extra's key paths and sizes, never its values.
#[tokio::test]
async fn gw_i_037_the_round_trip_carries_a_tool_calls_extra_and_shows_only_its_shape() -> TestResult
{
    const SIGNATURE: &str = "opaque-signature-marker";
    let piece = serde_json::json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function",
        "function":{"name":"probe_echo","arguments":serde_json::json!({"value":NONCE}).to_string()},
        "extra_content":{"google":{"thought_signature":SIGNATURE}}}]},"finish_reason":"tool_calls"}]});
    let server = start_model(vec![
        Script::new(vec![Scripted::RawJson(piece.to_string()), usage()]),
        Script::new(vec![Scripted::Text("ok".into()), usage()]),
    ])
    .await?;
    // Strict off: this case is about the extra, so the probe makes no strict-string check (GW-U-064).
    let route = route("openai_chat", server.base_url(), "strict='off'")?;
    let observed = probe(&route, limiter(&route)).await?;
    assert!(observed.ok, "{observed:?}");
    let second = &server.requests()[1].body;
    assert_eq!(
        second["messages"][2]["tool_calls"][0]["extra_content"]["google"]["thought_signature"],
        SIGNATURE,
        "{second}"
    );
    let shown = serde_json::to_string(&observed)?;
    assert!(!shown.contains(SIGNATURE), "{shown}");
    assert_eq!(
        observed.tool_call_extras,
        vec![format!(
            "extra_content.google.thought_signature: {} B",
            SIGNATURE.len() + 2
        )]
    );
    server.shutdown().await?;
    Ok(())
}

/// GW-U-079 (Gate 5, 29 Sep): on a text-protocol route, a server that holds part of the answer back (its own
/// tool-call parser takes a call the model wrote natively) is flagged with a warning that suggests native tool calls
/// for the route. Tasks recover from it with one retry per step, so the check still passes; a plain answer isn't flagged.
#[tokio::test]
async fn gw_u_079_a_text_route_whose_server_holds_calls_back_is_flagged() -> TestResult {
    let withheld =
        || Scripted::RawJson(r#"{"choices":[{"index":0,"delta":{},"finish_reason":null}]}"#.into());
    for (script, expected) in [
        // A sentence, then the call the server took.
        (
            vec![
                Scripted::Text("I'll call it now.".into()),
                withheld(),
                withheld(),
                withheld(),
                usage(),
            ],
            vec!["swallowed_calls"],
        ),
        // Nothing at all but the held-back chunks: flagged, not a protocol error.
        (
            vec![withheld(), withheld(), usage()],
            vec!["swallowed_calls"],
        ),
        // A plain answer holds nothing back.
        (vec![Scripted::Text("ok".into()), usage()], vec![]),
    ] {
        let server = start_model(vec![Script::new(script)]).await?;
        let text = route("openai_chat", server.base_url(), "native_tools=false\n")?;
        let observed = probe(&text, limiter(&text)).await?;
        assert!(observed.ok, "{observed:?}");
        assert_eq!(observed.warnings, expected);
        server.shutdown().await?;
    }
    Ok(())
}

/// The probe with its relay settings, key and cancellation given, for the failures that depend on them.
pub(super) async fn probe_with(
    route: &Route,
    config: &RelayConfig,
    secret: Option<&str>,
    cancel: &CancellationToken,
) -> Result<super::probe::Observation, Box<dyn std::error::Error>> {
    let until = super::probe::deadline(route, config);
    probe_until(route, config, secret, cancel, until).await
}

/// [`probe_with`] with the whole check's deadline given, to reach it while a model is still answering.
pub(super) async fn probe_until(
    route: &Route,
    config: &RelayConfig,
    secret: Option<&str>,
    cancel: &CancellationToken,
    until: std::time::Duration,
) -> Result<super::probe::Observation, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()?;
    let probe = Probe {
        client: &client,
        config,
        limiter: limiter(route),
        secret: secret.map(|value| secrecy::SecretString::from(value.to_owned())),
        nonce: NONCE.into(),
    };
    Ok(super::probe::run_until(route, probe, cancel, until).await)
}

#[tokio::test]
async fn gw_i_037_an_unusable_tool_call_is_named() -> TestResult {
    let code = |observed: &super::probe::Observation| {
        observed.failure.as_ref().map(|failure| failure.code)
    };
    // A tool call the probe can't use: arguments that are JSON but not an object, or a call to another tool. Both are
    // bad_tool_call, the call marked failed.
    for (name, arguments) in [
        ("probe_echo", "[1, 2]"),
        ("another_tool", r#"{"value": "x"}"#),
    ] {
        let unusable = Scripted::ToolCall {
            name: name.into(),
            arguments_json: arguments.into(),
            split_into: 1,
        };
        let server = start_model(vec![Script::new(vec![unusable, usage()])]).await?;
        let bad = route("openai_chat", server.base_url(), "")?;
        let observed = probe(&bad, limiter(&bad)).await?;
        assert_eq!(
            (code(&observed), observed.tool_call),
            (Some("bad_tool_call"), Check::Failed),
            "{name} {arguments}"
        );
        assert!(!observed.ok);
        server.shutdown().await?;
    }
    // Arguments the stream can't assemble into JSON, whether broken or cut off at the output limit, are the model's
    // tool call, not the server's protocol: bad_tool_call, the call marked failed (a local model's broken JSON).
    for events in [
        vec![
            Scripted::ToolCall {
                name: "probe_echo".into(),
                arguments_json: "{not json".into(),
                split_into: 1,
            },
            usage(),
        ],
        vec![
            Scripted::ToolCall {
                name: "probe_echo".into(),
                arguments_json: r#"{"value": "n0n"#.into(),
                split_into: 1,
            },
            Scripted::Finish(tqa_llm::FinishReason::Length),
            usage(),
        ],
    ] {
        let server = start_model(vec![Script::new(events)]).await?;
        let broken = route("openai_chat", server.base_url(), "")?;
        let observed = probe(&broken, limiter(&broken)).await?;
        assert_eq!(
            (code(&observed), observed.tool_call),
            (Some("bad_tool_call"), Check::Failed),
            "{observed:?}"
        );
        server.shutdown().await?;
    }

    Ok(())
}

#[tokio::test]
async fn gw_i_037_a_slow_server_a_cancelled_check_and_an_unsendable_key_are_named() -> TestResult {
    let code = |observed: &super::probe::Observation| {
        observed.failure.as_ref().map(|failure| failure.code)
    };
    // A server slower than the relay's header deadline: timeout.
    let mut slow = Script::new(vec![call(NONCE), usage()]);
    slow.header_delay = Duration::from_secs(3);
    let server = start_model(vec![slow]).await?;
    let late = route("openai_chat", server.base_url(), "")?;
    // The fake model is on this computer, so its route is local: its own limit is the one that runs out.
    let config = RelayConfig {
        header_timeout: 1,
        local_check_timeout: 1,
        ..RelayConfig::default()
    };
    let observed = probe_with(&late, &config, None, &CancellationToken::new()).await?;
    assert_eq!(code(&observed), Some("timeout"));
    server.shutdown().await?;

    // The gateway shutting down during a check: cancelled, and the model is never asked.
    let server = start_model(vec![]).await?;
    let stopped = route("openai_chat", server.base_url(), "")?;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let observed = probe_with(&stopped, &RelayConfig::default(), None, &cancel).await?;
    assert_eq!(code(&observed), Some("cancelled"));
    assert!(server.requests().is_empty());

    // A key that can't be sent (pasted with a line break): configuration, and nothing is sent.
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='keyed'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='m'\nauth={{kind='bearer'}}\n\
         key_ref='secrets/local'\nmax_output=4096\nstream_usage=true",
        server.base_url()
    ))?;
    let keyed = registry.get("keyed").ok_or("route")?.clone();
    let observed = probe_with(
        &keyed,
        &RelayConfig::default(),
        Some("synthetic-key\nsecond line"),
        &CancellationToken::new(),
    )
    .await?;
    assert_eq!(code(&observed), Some("configuration"));
    assert!(server.requests().is_empty());
    server.shutdown().await?;
    Ok(())
}

/// A route to `base_url`, keyed (auth bearer on a shared key) or not.
fn routed(base_url: &str, keyed: bool) -> Result<Route, Box<dyn std::error::Error>> {
    let auth = if keyed {
        "auth={kind='bearer'}\nkey_ref='secrets/model'"
    } else {
        "auth={kind='none'}"
    };
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='r'\nprotocol='openai_chat'\nbase_url='{base_url}'\nmodel='m'\n{auth}\nmax_output=4096"
    ))?;
    Ok(registry.get("r").ok_or("route")?.clone())
}

#[test]
fn gw_u_092_a_local_route_gets_longer_to_answer_its_check() -> TestResult {
    // A model on this computer (loopback, localhost, the container host), on the owner's network, or one without a key:
    // a CPU model can take tens of seconds to answer (seen with a local 4B model), so each answer's first byte waits 2
    // minutes by default (`[relay] local_check_timeout`), and the whole check may run 10 minutes while the model keeps
    // answering (a first-hour check before 17.0; probe_slow_tests.rs).
    let config = RelayConfig::default();
    for (base_url, keyed) in [
        ("http://127.0.0.1:11434/v1", true),
        ("http://localhost:8080/v1", true),
        ("http://[::1]:8080/v1", true),
        ("http://host.docker.internal:11434/v1", true),
        ("http://host.containers.internal:11434/v1", true),
        ("http://models.lan:8080/v1", false),
        // On the owner's network with a key (Ollama as a compose service, a GPU box, Tailscale): a local model too.
        ("http://ollama:11434/v1", true),
        ("http://gpu-box.local:11434/v1", true),
        ("http://models.corp.internal/v1", true),
        ("http://192.168.1.1:11434/v1", true),
        ("http://100.64.0.1:11434/v1", true),
        ("http://[fd12:3456::1]:8000/v1", true),
    ] {
        let route = routed(base_url, keyed)?;
        assert_eq!(
            super::probe::deadline(&route, &config),
            Duration::from_mins(10),
            "{base_url}"
        );
        assert_eq!(
            super::probe::first_byte(&route, &config),
            Duration::from_mins(2),
            "{base_url}"
        );
    }
    // A keyed service elsewhere keeps the minute, and the relay's own wait for a first byte.
    for base_url in [
        "https://models.example/v1",
        "http://203.0.113.9/v1",
        "http://100.128.0.0/v1",
    ] {
        let route = routed(base_url, true)?;
        assert_eq!(
            super::probe::deadline(&route, &config),
            Duration::from_mins(1),
            "{base_url}"
        );
    }
    let hosted = routed("https://api.example.com/v1", true)?;
    assert_eq!(
        super::probe::deadline(&hosted, &config),
        Duration::from_mins(1)
    );
    assert_eq!(
        super::probe::first_byte(&hosted, &config),
        Duration::from_mins(1)
    );
    // The API's own limit always outlasts the check's, so running out of time is the check's named result, never a 504.
    for route in [routed("http://127.0.0.1:11434/v1", true)?, hosted] {
        assert!(
            super::probe_api::request_limit(&route, &config)
                > super::probe::deadline(&route, &config)
        );
        // And the router's backstop outlasts every route's own limit (GW-I-153).
        assert!(
            super::probe_api::request_ceiling(&config)
                > super::probe_api::request_limit(&route, &config)
        );
    }
    Ok(())
}

/// A Chat route as an administrator adds it, without saying anything about usage in the stream.
fn unset_route(base_url: &str) -> Result<Route, Box<dyn std::error::Error>> {
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='probed'\nprotocol='openai_chat'\nbase_url='{base_url}'\nmodel='m'\nauth={{kind='none'}}\n\
         max_output=4096\n"
    ))?;
    Ok(registry.get("probed").ok_or("route")?.clone())
}

// TEST: GW-U-149
#[tokio::test]
async fn gw_u_149_the_check_asks_for_usage_as_a_run_does() -> TestResult {
    let server = start_model(good()).await?;
    let unset = unset_route(server.base_url())?;
    let observed = probe(&unset, limiter(&unset)).await?;
    assert!(observed.ok && observed.usage.reported, "{observed:?}");
    assert!(observed.warnings.is_empty(), "{observed:?}");
    for request in server.requests() {
        assert_eq!(
            request.body["stream_options"],
            serde_json::json!({"include_usage": true})
        );
    }
    server.shutdown().await?;
    Ok(())
}

// TEST: GW-U-149
#[tokio::test]
async fn gw_u_149_a_server_refusing_stream_options_is_asked_again_without_it() -> TestResult {
    let refused = Scripted::Status {
        code: 400,
        body: r#"{"error":{"message":"Unrecognized request argument supplied: stream_options"}}"#
            .into(),
        retry_after: None,
    };
    let mut scripts = vec![Script::new(vec![refused])];
    scripts.extend([
        Script::new(vec![call(NONCE)]),
        Script::new(vec![Scripted::Text("ok".into())]),
        Script::new(vec![lines(&format!("{NONCE}\n{NONCE}"))]),
    ]);
    let server = start_model(scripts).await?;
    let unset = unset_route(server.base_url())?;
    let observed = probe(&unset, limiter(&unset)).await?;
    assert!(
        !observed.ok,
        "the route's tasks would be refused: not verified, {observed:?}"
    );
    let failure = observed.failure.as_ref().ok_or("no failure")?;
    assert_eq!((failure.code, failure.status), ("usage_refused", Some(400)));
    let suggestion = failure.suggestion.as_ref().ok_or("no suggestion")?;
    assert_eq!(
        (suggestion.field, suggestion.value.as_ref()),
        ("stream_usage", "false"),
        "one click turns it off"
    );
    assert!(
        observed.warnings.is_empty(),
        "the failure says it; no usage_missing beside it: {observed:?}"
    );
    let requests = server.requests();
    assert_eq!(
        requests.len(),
        4,
        "the refused request once more, then the check's other two"
    );
    assert!(requests[0].body.get("stream_options").is_some());
    assert!(
        requests[1..]
            .iter()
            .all(|request| request.body.get("stream_options").is_none()),
        "every later request of the check goes without it"
    );
    server.shutdown().await?;
    // A 400 about something else is the route's failure, never retried.
    let other = Scripted::Status {
        code: 400,
        body: r#"{"error":"bad model"}"#.into(),
        retry_after: None,
    };
    let server = start_model(vec![Script::new(vec![other])]).await?;
    let unset = unset_route(server.base_url())?;
    let observed = probe(&unset, limiter(&unset)).await?;
    assert!(!observed.ok && observed.warnings.is_empty(), "{observed:?}");
    assert_eq!(server.requests().len(), 1);
    server.shutdown().await?;
    Ok(())
}
