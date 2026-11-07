//! CORE-E-009/011 and CORE-U-005: pure provider scripts, fake latency and stream ownership.
use futures::StreamExt;
use std::{error::Error, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_llm::{
    CompletionRequest, FinishReason, LlmError, ModelProvider, Registry, Route, StreamChunk,
    StreamLimits, collect,
};
use tqa_testkit::{FakeClock, FakeProvider, FakeSleeper, ProviderChunk, ProviderReply};

fn route() -> Result<Route, Box<dyn Error>> {
    Registry::from_toml(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='example-model'\n",
    )?
    .get("local")
    .cloned()
    .ok_or_else(|| "missing fake route".into())
}

fn chunk(result: StreamChunk) -> ProviderChunk {
    ProviderChunk {
        delay: Duration::ZERO,
        result: Ok(result),
    }
}

fn reply(chunks: Vec<ProviderChunk>) -> ProviderReply {
    ProviderReply {
        delay: Duration::ZERO,
        result: Ok(chunks),
    }
}

#[tokio::test]
async fn core_u_005_precancel_preserves_script_and_records_request() -> Result<(), Box<dyn Error>> {
    let provider = FakeProvider::new(
        route()?,
        vec![reply(vec![chunk(StreamChunk::Finish(FinishReason::Stop))])],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        provider
            .complete(CompletionRequest::default(), cancel)
            .await,
        Err(LlmError::Cancelled)
    ));
    assert_eq!(provider.requests().len(), 1);
    let cancel = CancellationToken::new();
    let mut stream = provider
        .complete(CompletionRequest::default(), cancel.clone())
        .await?;
    cancel.cancel();
    assert!(matches!(
        stream.next().await,
        Some(Err(LlmError::Cancelled))
    ));
    assert!(stream.next().await.is_none());
    assert_eq!(provider.active_streams(), 0);
    Ok(())
}

#[tokio::test]
async fn core_e_011_stream_failure_discards_later_scripted_chunks() -> Result<(), Box<dyn Error>> {
    let provider = FakeProvider::new(
        route()?,
        vec![reply(vec![
            chunk(StreamChunk::TextDelta("partial".into())),
            ProviderChunk {
                delay: Duration::ZERO,
                result: Err(LlmError::Io("injected failure".into())),
            },
            chunk(StreamChunk::TextDelta("must not escape".into())),
        ])],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    );
    let mut stream = provider
        .complete(CompletionRequest::default(), CancellationToken::new())
        .await?;
    assert!(
        matches!(stream.next().await, Some(Ok(StreamChunk::TextDelta(text))) if text == "partial")
    );
    assert!(matches!(stream.next().await, Some(Err(LlmError::Io(_)))));
    assert!(stream.next().await.is_none());
    assert_eq!(provider.active_streams(), 0);
    Ok(())
}

#[tokio::test]
async fn core_e_009_provider_records_status_failure_and_success() -> Result<(), Box<dyn Error>> {
    let provider = FakeProvider::new(
        route()?,
        vec![
            ProviderReply {
                delay: Duration::ZERO,
                result: Err(LlmError::Http {
                    status: 503,
                    body_excerpt: "temporarily unavailable".into(),
                }),
            },
            reply(vec![
                chunk(StreamChunk::TextDelta("answer".into())),
                chunk(StreamChunk::Finish(FinishReason::Stop)),
            ]),
        ],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    );
    let request = CompletionRequest {
        max_output: Some(123),
        ..CompletionRequest::default()
    };
    let first = provider
        .complete(request.clone(), CancellationToken::new())
        .await;
    assert!(matches!(first, Err(LlmError::Http { status: 503, .. })));
    let next_request = CompletionRequest {
        max_output: Some(456),
        ..request.clone()
    };
    let second = provider
        .complete(next_request.clone(), CancellationToken::new())
        .await?;
    assert_eq!(
        collect(second, StreamLimits::RECOMMENDED).await?.text,
        "answer"
    );
    assert_eq!(provider.requests(), vec![request, next_request]);
    assert_eq!(provider.route(), &route()?);
    assert_eq!(provider.active_streams(), 0);
    assert!(matches!(
        provider
            .complete(CompletionRequest::default(), CancellationToken::new())
            .await,
        Err(LlmError::Protocol(_))
    ));
    assert_eq!(provider.requests().len(), 3);
    Ok(())
}

#[tokio::test]
async fn core_u_005_provider_waits_only_for_injected_time() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let provider = FakeProvider::new(
        route()?,
        vec![ProviderReply {
            delay: Duration::from_millis(10),
            result: Ok(vec![ProviderChunk {
                delay: Duration::from_millis(20),
                result: Ok(StreamChunk::TextDelta("after delay".into())),
            }]),
        }],
        Arc::new(FakeSleeper::new(clock.clone())),
    );
    let complete = provider.complete(CompletionRequest::default(), CancellationToken::new());
    tokio::pin!(complete);
    assert!(futures::poll!(complete.as_mut()).is_pending());
    clock.advance(Duration::from_millis(10))?;
    let mut stream = tokio_test::assert_ready!(futures::poll!(complete.as_mut()))?;
    assert_eq!(provider.active_streams(), 1);
    let mut next = stream.next();
    tokio_test::assert_pending!(futures::poll!(&mut next));
    clock.advance(Duration::from_millis(19))?;
    tokio_test::assert_pending!(futures::poll!(&mut next));
    clock.advance(Duration::from_millis(1))?;
    let result = tokio_test::assert_ready!(futures::poll!(&mut next));
    assert!(matches!(result, Some(Ok(StreamChunk::TextDelta(text))) if text == "after delay"));
    drop(next);
    drop(stream);
    assert_eq!(provider.active_streams(), 0);
    Ok(())
}

#[tokio::test]
async fn core_e_011_provider_can_cut_before_finish() -> Result<(), Box<dyn Error>> {
    let provider = FakeProvider::new(
        route()?,
        vec![reply(vec![chunk(StreamChunk::TextDelta("partial".into()))])],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    );
    let stream = provider
        .complete(CompletionRequest::default(), CancellationToken::new())
        .await?;
    let result = collect(stream, StreamLimits::RECOMMENDED).await;
    assert!(matches!(result, Err(LlmError::StreamCut { partial }) if partial.text == "partial"));
    assert_eq!(provider.active_streams(), 0);
    Ok(())
}

#[tokio::test]
async fn core_u_005_provider_cancellation_closes_pending_stream() -> Result<(), Box<dyn Error>> {
    let provider = FakeProvider::new(
        route()?,
        vec![reply(vec![ProviderChunk {
            delay: Duration::from_mins(1),
            result: Ok(StreamChunk::TextDelta("never sent".into())),
        }])],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    );
    let cancel = CancellationToken::new();
    let mut stream = provider
        .complete(CompletionRequest::default(), cancel.clone())
        .await?;
    let mut next = stream.next();
    tokio_test::assert_pending!(futures::poll!(&mut next));
    cancel.cancel();
    assert!(matches!(
        tokio_test::assert_ready!(futures::poll!(&mut next)),
        Some(Err(LlmError::Cancelled))
    ));
    drop(next);
    assert!(stream.next().await.is_none());
    drop(stream);
    assert_eq!(provider.active_streams(), 0);
    Ok(())
}

#[tokio::test]
async fn core_u_005_provider_cancellation_interrupts_response_delay() -> Result<(), Box<dyn Error>>
{
    let provider = FakeProvider::new(
        route()?,
        vec![ProviderReply {
            delay: Duration::from_mins(1),
            result: Ok(vec![]),
        }],
        Arc::new(FakeSleeper::new(FakeClock::new())),
    );
    let cancel = CancellationToken::new();
    let complete = provider.complete(CompletionRequest::default(), cancel.clone());
    tokio::pin!(complete);
    assert!(futures::poll!(complete.as_mut()).is_pending());
    cancel.cancel();
    assert!(matches!(
        tokio_test::assert_ready!(futures::poll!(complete.as_mut())),
        Err(LlmError::Cancelled)
    ));
    assert_eq!(provider.active_streams(), 0);
    Ok(())
}
