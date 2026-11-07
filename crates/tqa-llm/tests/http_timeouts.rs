//! LLM-E-018: separate deadlines and cancellation, without real-time sleeps.

#[path = "fixtures/http.rs"]
mod fixtures;
#[path = "fixtures/observed_sleep.rs"]
mod observed_sleep;
use futures::StreamExt;
use std::{
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use tqa_llm::{HttpConfig, LlmError, StreamLimits, TimeoutKind, provider_for};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, Script, Scripted};

#[tokio::test]
async fn llm_e_018_first_byte_timeout_uses_manual_time() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let mut script = Script::new(vec![]);
    script.header_delay = Duration::from_mins(1);
    let server =
        FakeModelServer::start(vec![script], Arc::new(FakeSleeper::new(clock.clone()))).await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let task = tokio::spawn(async move {
        provider
            .complete(fixtures::request(), CancellationToken::new())
            .await
    });
    server.wait_for_requests(1).await?;
    clock.advance(Duration::from_secs(30))?;
    assert!(matches!(
        task.await?,
        Err(LlmError::Timeout(TimeoutKind::FirstByte))
    ));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_e_018_idle_timeout_uses_manual_time() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(
        vec![Script::new(vec![
            Scripted::Text("first".into()),
            Scripted::Delay(Duration::from_mins(2)),
            Scripted::Text("late".into()),
        ])],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let mut stream = provider
        .complete(fixtures::request(), CancellationToken::new())
        .await?;
    assert!(stream.next().await.transpose()?.is_some());
    let mut next = tokio_test::task::spawn(stream.next());
    assert!(next.poll().is_pending());
    clock.advance(Duration::from_mins(1))?;
    assert!(matches!(
        next.await,
        Some(Err(LlmError::Timeout(TimeoutKind::Idle)))
    ));
    drop(stream);
    server.shutdown().await?;
    Ok(())
}

/// How long a cancellation may take to end a stream. The server's next bytes wait on a fake clock nobody advances, so
/// a cancellation that isn't observed never ends the stream at all: a few seconds tells the two apart on any host
/// (200 ms failed on loaded ones).
const PROMPT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn llm_e_018_cancellation_is_prompt_during_a_stream() -> Result<(), Box<dyn Error>> {
    let clock = FakeClock::new();
    let server = FakeModelServer::start(
        vec![Script::new(vec![
            Scripted::Text("first".into()),
            Scripted::Delay(Duration::from_mins(2)),
        ])],
        Arc::new(FakeSleeper::new(clock.clone())),
    )
    .await?;
    let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
    let cancel = CancellationToken::new();
    let mut request = fixtures::request();
    request.tools.push(fixtures::tool());
    let mut stream = provider.complete(request, cancel.clone()).await?;
    assert!(stream.next().await.transpose()?.is_some());
    let started = Instant::now();
    cancel.cancel();
    assert!(matches!(
        tokio::time::timeout(PROMPT, stream.next()).await?,
        Some(Err(LlmError::Cancelled))
    ));
    assert!(started.elapsed() < PROMPT);
    drop(stream);
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn llm_e_018_connect_timeout_during_stalled_local_tls_handshake() -> Result<(), Box<dyn Error>>
{
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let (accepted, received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        let _ = accepted.send(());
        stop.cancelled().await;
        drop(socket);
        Ok::<(), std::io::Error>(())
    });
    let clock = FakeClock::new();
    let provider = provider_for(
        fixtures::route(&format!("https://{address}/v1"))?,
        None,
        HttpConfig::new(StreamLimits::RECOMMENDED),
        Arc::new(clock.clone()),
        Arc::new(FakeSleeper::new(clock)),
    )?;
    let task = tokio::spawn(async move {
        provider
            .complete(fixtures::request(), CancellationToken::new())
            .await
    });
    received.await?;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(11)).await;
    assert!(matches!(
        task.await?,
        Err(LlmError::Timeout(TimeoutKind::Connect))
    ));
    cancel.cancel();
    server.await??;
    Ok(())
}

#[tokio::test]
async fn llm_e_018_drop_and_unpolled_cancellation_release_the_response()
-> Result<(), Box<dyn Error>> {
    for cancel_only in [false, true] {
        let clock = FakeClock::new();
        let observer = Arc::new(observed_sleep::ObservedSleep::new(clock.clone()));
        let server = FakeModelServer::start(
            vec![Script::new(vec![
                Scripted::Text("first".into()),
                Scripted::Delay(Duration::from_mins(2)),
            ])],
            observer.clone(),
        )
        .await?;
        let provider = fixtures::provider(fixtures::route(server.base_url())?, &clock)?;
        let cancel = CancellationToken::new();
        let mut stream = provider
            .complete(fixtures::request(), cancel.clone())
            .await?;
        assert!(stream.next().await.transpose()?.is_some());
        tokio::time::timeout(PROMPT, observer.wait(true)).await??;
        let started = Instant::now();
        if cancel_only {
            cancel.cancel();
            tokio::time::timeout(PROMPT, observer.wait(false)).await??;
            assert!(matches!(
                tokio::time::timeout(PROMPT, stream.next()).await?,
                Some(Err(LlmError::Cancelled))
            ));
        } else {
            drop(stream);
            tokio::time::timeout(PROMPT, observer.wait(false)).await??;
        }
        assert!(started.elapsed() < PROMPT);
        server.shutdown().await?;
    }
    Ok(())
}
