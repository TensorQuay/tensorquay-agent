//! RT-I-001/002: the fake enforces binding and stores contiguous frames before replay. TK-U-028: its path switch.
use futures::{SinkExt as _, StreamExt as _};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest as _};
use tqa_testkit::{FakeClock, FakeGateway, FakeGatewayOptions, FakeSleeper};
use tqa_types::{ClientFrame, ErrorCode, ServerFrame};

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn session() -> Result<ClientFrame, Box<dyn std::error::Error>> {
    Ok(ClientFrame::parse(serde_json::to_string(&serde_json::json!({
        "type":"session.config","conversation_id":"conversation","last_stored_seq":0,
        "route":{"name":"local","protocol":"openai_chat","model":"fake-model","relay_path":"/relay/r/local",
            "context_window":32768,"max_output":4096,"native_tools":true,"parallel_tools":true,"vision":false,"reasoning":null},
        "network":{"allowed":[],"more":0},"limits":{"max_steps":10,"wall_clock_secs":300,"tool_result_bytes":8192},
        "workspace":{"display":"/workspace"},"protocol":1
    }))?.as_bytes())?)
}

#[tokio::test]
async fn rt_i_001_fake_gateway_binding_registration_and_history() -> TestResult {
    let clock = FakeClock::new();
    let gateway = FakeGateway::start(FakeGatewayOptions {
        token: "test-runtime-credential".into(),
        session: session()?,
        history: vec![],
        scripts: vec![],
        sleeper: Arc::new(FakeSleeper::new(clock)),
    })
    .await?;
    let request = || -> Result<_, Box<dyn std::error::Error>> {
        let mut request = gateway.link_url().into_client_request()?;
        request
            .headers_mut()
            .insert("authorization", "Bearer test-runtime-credential".parse()?);
        Ok(request)
    };
    assert!(tokio_tungstenite::connect_async(request()?).await.is_err());
    gateway.set_bound(true);
    assert!(gateway.set_frame_limit(0).is_err());
    assert!(gateway.set_frame_limit(131_073).is_err());
    gateway.set_frame_limit(1)?;
    let (mut socket, _) = tokio_tungstenite::connect_async(request()?).await?;
    let first = socket.next().await.ok_or("closed")??.into_text()?;
    assert!(matches!(
        ClientFrame::parse(first.as_bytes())?,
        ClientFrame::SessionConfig { .. }
    ));
    let frame = ServerFrame::Error {
        target: None,
        seq: Some(1.into()),
        code: ErrorCode::Io,
        message: "test".into(),
        retryable: false,
    };
    socket
        .send(Message::Text(serde_json::to_string(&frame)?.into()))
        .await?;
    gateway.wait_for_frames(1).await?;
    assert_eq!(gateway.stored(), vec![frame]);
    let page: serde_json::Value = reqwest::Client::new()
        .get(format!(
            "{}/runtime/history?after=0&limit=100",
            gateway.origin()
        ))
        .bearer_auth("test-runtime-credential")
        .send()
        .await?
        .json()
        .await?;
    assert_eq!(page["next_after"], 1);
    assert_eq!(page["items"].as_array().ok_or("items")?.len(), 1);
    socket.close(None).await?;
    gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_002_fake_gateway_refuses_duplicate_or_gapped_frames() -> TestResult {
    let gateway = FakeGateway::start(FakeGatewayOptions {
        token: "test-runtime-credential".into(),
        session: session()?,
        history: vec![],
        scripts: vec![],
        sleeper: Arc::new(tqa_llm::TokioSleeper),
    })
    .await?;
    gateway.set_bound(true);
    let mut request = gateway.link_url().into_client_request()?;
    request
        .headers_mut()
        .insert("authorization", "Bearer test-runtime-credential".parse()?);
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
    socket.next().await.ok_or("closed")??;
    let frame = ServerFrame::Error {
        target: None,
        seq: Some(2.into()),
        code: ErrorCode::Io,
        message: "test".into(),
        retryable: false,
    };
    socket
        .send(Message::Text(serde_json::to_string(&frame)?.into()))
        .await?;
    let result = tokio::time::timeout(
        tqa_testkit::deadline::scaled(std::time::Duration::from_secs(5)),
        socket.next(),
    )
    .await?;
    assert!(result.is_none_or(|value| value.is_err() || matches!(value, Ok(Message::Close(_)))));
    assert!(gateway.stored().is_empty());
    assert_eq!(gateway.frames(), vec![frame]);
    gateway.shutdown().await?;
    Ok(())
}

// TEST: TK-U-028 (the fake's path switch, for the runtime's link-outage test)
#[tokio::test]
async fn tk_u_028_a_cut_path_fails_requests_cuts_streams_silences_the_link_and_comes_back()
-> TestResult {
    use std::time::Duration;
    use tqa_testkit::{Script, Scripted};
    let clock = FakeClock::new();
    // The first answer sends its headers and a delta, then waits on a clock nobody advances: it is mid-stream.
    let stalled = Script::new(vec![
        Scripted::Text("before".into()),
        Scripted::Delay(Duration::from_hours(1)),
        Scripted::Text("after".into()),
    ]);
    let gateway = FakeGateway::start(FakeGatewayOptions {
        token: "test-runtime-credential".into(),
        session: session()?,
        history: vec![],
        scripts: vec![
            stalled,
            Script::new(vec![Scripted::Text("restored".into())]),
        ],
        sleeper: Arc::new(FakeSleeper::new(clock)),
    })
    .await?;
    gateway.set_bound(true);
    let dial = || -> Result<_, Box<dyn std::error::Error>> {
        let mut request = gateway.link_url().into_client_request()?;
        request
            .headers_mut()
            .insert("authorization", "Bearer test-runtime-credential".parse()?);
        Ok(request)
    };
    let (mut link, _) = tokio_tungstenite::connect_async(dial()?).await?;
    link.next().await.ok_or("closed")??;
    let client = reqwest::Client::new();
    let relay = format!("{}/relay/r/local/chat/completions", gateway.origin());
    let post = || {
        client
            .post(&relay)
            .bearer_auth("test-runtime-credential")
            .json(&serde_json::json!({"model":"fake-model","messages":[],"stream":true}))
            .send()
    };
    let streaming = post().await?;
    assert!(streaming.status().is_success());
    let mut body = streaming.bytes_stream();
    let first = body.next().await.ok_or("no first chunk")??;
    assert!(String::from_utf8_lossy(&first).contains("before"));

    gateway.set_path_down(true);
    // (1) The stream already flowing is cut mid-body.
    let rest: Vec<_> = tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        body.collect(),
    )
    .await?;
    let cut = rest.iter().any(Result::is_err);
    assert!(cut, "the body fails; it doesn't just end");
    // (2) A new request fails before any response headers.
    let refused = post().await;
    assert!(refused.is_err(), "no response while the path is down");
    // (3) The registered link goes silent: no pong, no close frame.
    link.send(Message::Ping(b"probe".to_vec().into())).await?;
    assert!(
        tokio::time::timeout(Duration::from_millis(300), link.next())
            .await
            .is_err(),
        "nothing answers while the path is down"
    );
    // (4) Redials are refused.
    assert!(tokio_tungstenite::connect_async(dial()?).await.is_err());

    gateway.set_path_down(false);
    // The silent link goes without a close frame, a redial registers, and requests work again.
    let gone = tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        link.next(),
    )
    .await?;
    assert!(
        !matches!(gone, Some(Ok(Message::Close(_)))),
        "no close handshake: {gone:?}"
    );
    let (mut again, _) = tokio_tungstenite::connect_async(dial()?).await?;
    again.next().await.ok_or("closed")??;
    gateway.wait_for_registrations(2).await?;
    let answer = post().await?.text().await?;
    assert!(answer.contains("restored"), "{answer}");
    again.close(None).await?;
    gateway.shutdown().await?;
    Ok(())
}

// TEST: RT-I-033 (the fake's store stall: a malformed frame still ends the link)
#[tokio::test]
async fn rt_i_033_a_malformed_frame_ends_the_link_while_the_store_is_stalled() -> TestResult {
    let gateway = FakeGateway::start(FakeGatewayOptions {
        token: "test-runtime-credential".into(),
        session: session()?,
        history: vec![],
        scripts: vec![],
        sleeper: Arc::new(tqa_llm::TokioSleeper),
    })
    .await?;
    gateway.set_bound(true);
    let mut request = gateway.link_url().into_client_request()?;
    request
        .headers_mut()
        .insert("authorization", "Bearer test-runtime-credential".parse()?);
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
    socket.next().await.ok_or("closed")??;
    gateway.set_store_stalled(true);
    socket.send(Message::Text("not a frame".into())).await?;
    let result = tokio::time::timeout(
        tqa_testkit::deadline::scaled(std::time::Duration::from_secs(5)),
        socket.next(),
    )
    .await?;
    assert!(result.is_none_or(|value| value.is_err() || matches!(value, Ok(Message::Close(_)))));
    gateway.set_store_stalled(false);
    assert!(gateway.frames().is_empty(), "nothing was held to record");
    gateway.shutdown().await?;
    Ok(())
}
