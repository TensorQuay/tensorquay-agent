//! LLM-E-006/007/016 and LLM-U-017: fragmented streams, cuts and byte-stable requests.

#[path = "fixtures/http.rs"]
mod fixtures;
use std::{error::Error, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_llm::{HttpConfig, LlmError, StreamLimits, collect, provider_for};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, MalformedSse, Script, Scripted};

#[tokio::test]
async fn llm_e_006_sse_edge_cases_match_the_clean_stream() -> Result<(), Box<dyn Error>> {
    let variants = [
        MalformedSse::SplitUtf8,
        MalformedSse::SplitData,
        MalformedSse::CrLf,
        MalformedSse::Comments,
        MalformedSse::MissingDone,
    ];
    let mut scripts = vec![Script::new(vec![Scripted::Text("hello界🙂".into())])];
    scripts.extend(variants.into_iter().map(|variant| {
        Script::new(vec![
            Scripted::Text("hello界🙂".into()),
            Scripted::MalformedSse(variant),
        ])
    }));
    let clock = FakeClock::new();
    let server = FakeModelServer::start(scripts, Arc::new(FakeSleeper::new(clock.clone()))).await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let baseline = collect(
        provider
            .complete(fixtures::request(), CancellationToken::new())
            .await?,
        StreamLimits::RECOMMENDED,
    )
    .await?;
    for _ in variants {
        let completion = collect(
            provider
                .complete(fixtures::request(), CancellationToken::new())
                .await?,
            StreamLimits::RECOMMENDED,
        )
        .await?;
        assert_eq!(completion, baseline);
    }
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_e_007_stream_cut_retains_partial_text() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(
        vec![Script::new(vec![
            Scripted::Text("first".into()),
            Scripted::Text("second".into()),
            Scripted::CutAfter(1),
        ])],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let result = collect(
        provider
            .complete(fixtures::request(), CancellationToken::new())
            .await?,
        StreamLimits::RECOMMENDED,
    )
    .await;
    assert!(matches!(result, Err(LlmError::StreamCut { partial }) if partial.text == "first"));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_e_016_http_body_cap_and_error_redaction() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let credential = format!("sk-{}", "demo".repeat(12));
    let server = FakeModelServer::start(
        vec![
            Script::new(vec![Scripted::Status {
                code: 401,
                body: format!("x-api-key: {credential}"),
                retry_after: None,
            }]),
            Script::new(vec![Scripted::Status {
                code: 500,
                body: "x".repeat(129),
                retry_after: None,
            }]),
        ],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let mut http = HttpConfig::new(StreamLimits::RECOMMENDED);
    http.body_cap = 128;
    let provider = provider_for(
        fixtures::route(server.base_url())?,
        None,
        http,
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock)),
    )?;
    let error = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await
        .err()
        .ok_or("accepted 401")?;
    assert!(matches!(error, LlmError::Http { status: 401, .. }));
    assert!(!format!("{error:?} {error}").contains(&credential));
    assert!(error.to_string().contains("[redacted]"));
    assert!(matches!(
        provider
            .complete(fixtures::request(), CancellationToken::new())
            .await,
        Err(LlmError::BodyTooLarge { max: 128 })
    ));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_u_017_request_prefix_is_byte_identical_across_twenty_calls()
-> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(
        vec![Script::new(vec![]); 20],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let mut request = fixtures::request();
    request.tools.push(fixtures::tool());
    for _ in 0..20 {
        collect(
            provider
                .complete(request.clone(), CancellationToken::new())
                .await?,
            StreamLimits::RECOMMENDED,
        )
        .await?;
    }
    let records = server.requests();
    let expected = serde_json::to_vec(&records[0].body)?;
    assert_eq!(records.len(), 20);
    for record in records {
        assert_eq!(record.encoded_body, expected);
    }
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_c_002_http_interleaves_parallel_calls_and_final_usage() -> Result<(), Box<dyn Error>> {
    let mut events = Vec::new();
    for (position, part) in ["{", "\"n\"", ":", "1", "}"].into_iter().enumerate() {
        for index in [9, 2] {
            events.push(Scripted::ToolCallDelta {
                index,
                id: (position == 0).then(|| format!("call-{index}")),
                name: (position == 0).then(|| "read_file".into()),
                arguments_delta: part.into(),
            });
        }
    }
    let usage = tqa_llm::Usage {
        prompt_tokens: 10,
        completion_tokens: 4,
        cached_tokens: Some(3),
        reasoning_tokens: None,
    };
    events.push(Scripted::Usage(usage.clone()));
    let clock = FakeClock::new();
    let server = FakeModelServer::start(
        vec![Script::new(events)],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let completion = collect(
        provider
            .complete(fixtures::request(), CancellationToken::new())
            .await?,
        StreamLimits::RECOMMENDED,
    )
    .await?;
    assert_eq!(
        completion
            .tool_calls
            .iter()
            .map(|call| (call.id.as_str(), call.arguments.as_str()))
            .collect::<Vec<_>>(),
        [("call-2", "{\"n\":1}"), ("call-9", "{\"n\":1}")]
    );
    assert_eq!(completion.usage, Some(usage));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_e_016_adapter_enforces_all_stream_limits() -> Result<(), Box<dyn Error>> {
    use tqa_llm::StreamLimitKind;
    let cases = [
        (
            StreamLimits {
                max_sse_event_bytes: 3,
                ..StreamLimits::RECOMMENDED
            },
            Scripted::Text("first".into()),
            StreamLimitKind::SseEvent,
        ),
        (
            StreamLimits {
                max_completion_bytes: 3,
                ..StreamLimits::RECOMMENDED
            },
            Scripted::Text("界x".into()),
            StreamLimitKind::Completion,
        ),
        (
            StreamLimits {
                max_tool_calls: 0,
                ..StreamLimits::RECOMMENDED
            },
            Scripted::ToolCall {
                name: "read_file".into(),
                arguments_json: "{}".into(),
                split_into: 1,
            },
            StreamLimitKind::ToolCalls,
        ),
    ];
    for (limits, event, expected) in cases {
        let clock = FakeClock::new();
        let server = FakeModelServer::start(
            vec![Script::new(vec![event])],
            Arc::new(FakeSleeper::new(clock.clone())),
        )
        .await?;
        let provider = provider_for(
            fixtures::route(server.base_url())?,
            None,
            HttpConfig::new(limits),
            Arc::new(clock.clone()),
            Arc::new(FakeSleeper::new(clock)),
        )?;
        let result = collect(
            provider
                .complete(fixtures::request(), CancellationToken::new())
                .await?,
            StreamLimits::RECOMMENDED,
        )
        .await;
        assert!(matches!(result, Err(LlmError::StreamLimit { which, .. }) if which == expected));
        server.shutdown().await?;
    }
    Ok(())
}
