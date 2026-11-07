//! RT-I-001/007: failed registration, corrupt history and unknown route protocols fail closed.
#[path = "support/fixture.rs"]
mod support;
use std::time::Duration;
use support::*;
use tqa_runtime::{Runtime, RuntimeError};
use tqa_testkit::{Script, Scripted};
use tqa_types::{ClientFrame, ErrorCode, Event, Seq, ServerFrame};

#[tokio::test]
async fn rt_i_001_unknown_configuration_field_reports_protocol_error_and_exits_two() -> TestResult {
    for nested in [false, true] {
        let f = Fixture::new(vec![answer()]).await?;
        let mut config = serde_json::to_value(session()?)?;
        let object = if nested {
            &mut config["limits"]
        } else {
            &mut config
        };
        object["future_output_cap"] = serde_json::json!("untrusted-fixture-value");
        f.gateway.first_frame(serde_json::to_string(&config)?)?;
        f.gateway.set_bound(true);
        let (shutdown, mut task) = f.launch()?;
        let result = tokio::time::timeout(
            tqa_testkit::deadline::scaled(Duration::from_secs(5)),
            &mut task,
        )
        .await;
        shutdown.cancel();
        let error = result??.err().ok_or("unknown limits must fail closed")?;
        assert_eq!(error.exit_code(), 2);
        wait_for(|| !f.gateway.frames().is_empty()).await?;
        let frames = f.gateway.frames();
        assert_eq!(frames.len(), 1);
        let ServerFrame::Error {
            target: None,
            seq,
            code,
            message,
            retryable,
        } = &frames[0]
        else {
            return Err("expected one unsequenced configuration error".into());
        };
        assert_eq!(*seq, None);
        assert_eq!(*code, ErrorCode::UnsupportedProtocol);
        assert!(!retryable);
        assert!(message.contains("future_output_cap"));
        assert!(!message.contains("untrusted-fixture-value"));
        assert!(f.gateway.stored().is_empty());
        assert!(f.gateway.model_requests().is_empty());
        assert!(
            !f.gateway
                .requests()
                .iter()
                .any(|r| r.path == "/runtime/history")
        );
        f.gateway.shutdown().await?;
    }
    Ok(())
}

#[tokio::test]
async fn rt_i_001_unbound_token_exits_three_at_the_sixty_second_deadline() -> TestResult {
    let f = Fixture::new(vec![]).await?;
    let (_, task) = f.launch()?;
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_millis(125))
    })
    .await?;
    f.clock.advance(Duration::from_mins(1))?;
    let error = tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task)
        .await??
        .err()
        .ok_or("unexpected registration")?;
    assert_eq!(error, RuntimeError::StartDeadline);
    assert_eq!(error.exit_code(), 3);
    assert_eq!(
        error.to_string(),
        "the gateway never accepted the runtime token"
    );
    assert!(f.gateway.stored().is_empty());
    f.gateway.shutdown().await?;
    Ok(())
}

/// Names the step whose bounded wait failed: each waits 5 s of real time for milliseconds of work, so a miss is a hang
/// at that step, not slowness (TEST-PLAN §19a, this test's timeout under load).
fn at<E: std::fmt::Display>(step: &'static str) -> impl FnOnce(E) -> Box<dyn std::error::Error> {
    move |error| format!("{step}: {error}").into()
}

#[tokio::test]
async fn rt_i_001_first_frame_must_be_session_config_before_history_or_actions() -> TestResult {
    let f = Fixture::new(vec![answer()]).await?;
    f.gateway.set_bound(true);
    f.gateway.first_frame(serde_json::to_string(&user())?)?;
    let (shutdown, task) = f.launch()?;
    f.gateway
        .wait_for_registrations(1)
        .await
        .map_err(at("the first registration"))?;
    wait_for(|| {
        f.sleeper
            .pending_durations()
            .contains(&Duration::from_millis(125))
    })
    .await
    .map_err(at("the refused link's 125 ms redial wait"))?;
    assert!(
        !f.gateway
            .requests()
            .iter()
            .any(|r| r.path == "/runtime/history")
    );
    assert!(f.gateway.model_requests().is_empty());
    f.clock.advance(Duration::from_millis(125))?;
    f.gateway
        .wait_for_registrations(2)
        .await
        .map_err(at("the redial's registration"))?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed")
        .await
        .map_err(at("run.completed"))?;
    stop(shutdown, task)
        .await
        .map_err(at("the runtime's stop"))?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_001_history_gap_is_fatal_and_never_calls_the_model() -> TestResult {
    let history = vec![ServerFrame::Event {
        seq: Seq::from(2),
        run_id: None,
        step: None,
        ts: "2000-01-01T00:00:00Z".into(),
        event: Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "question".into(),
            attachments: vec![],
        },
    }];
    let f = Fixture::with_history(vec![answer()], history).await?;
    f.gateway.set_bound(true);
    let (_, task) = f.launch()?;
    assert_eq!(
        tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task).await??,
        Err(RuntimeError::History)
    );
    assert!(f.gateway.model_requests().is_empty());
    assert!(f.gateway.frames().is_empty());
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_007_an_unknown_route_protocol_fails_closed_without_relay_traffic() -> TestResult {
    // A protocol this runtime cannot speak is refused when the configuration is parsed.
    let f = Fixture::new(vec![answer()]).await?;
    let mut config = serde_json::to_value(session()?)?;
    config["route"]["protocol"] = serde_json::json!("future_protocol");
    f.gateway.first_frame(serde_json::to_string(&config)?)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    let result =
        tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task).await;
    shutdown.cancel();
    let error = result??
        .err()
        .ok_or("an unknown protocol must fail closed")?;
    assert_eq!(error.exit_code(), 2, "a configuration error is not retried");
    let frames = f.gateway.frames();
    assert!(
        matches!(
            frames.as_slice(),
            [ServerFrame::Error { code: ErrorCode::UnsupportedProtocol, message, .. }]
                if message.starts_with("unsupported session.config")
        ),
        "{frames:?}"
    );
    assert!(f.gateway.model_requests().is_empty());
    assert!(f.gateway.stored().is_empty());
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_003_only_the_active_run_can_be_cancelled() -> TestResult {
    let f = Fixture::new(vec![Script::new(vec![
        Scripted::Text("partial".into()),
        Scripted::Delay(Duration::from_secs(20)),
    ])])
    .await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    let run_id = f
        .gateway
        .stored()
        .iter()
        .find_map(|frame| match frame {
            ServerFrame::Event {
                event: Event::RunStarted { run_id },
                ..
            } => Some(run_id.clone()),
            _ => None,
        })
        .ok_or("missing run")?;
    f.gateway.send(ClientFrame::RunCancel { run_id }).await?;
    f.wait_event("run.cancelled").await?;
    assert!(!f.gateway.stored().iter().any(|frame| matches!(
        frame,
        ServerFrame::Event {
            event: Event::RunFailed { .. },
            ..
        }
    )));
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_001_history_memory_cap_stops_before_a_partial_run_can_start() -> TestResult {
    let history = vec![ServerFrame::Event {
        seq: Seq::from(1),
        run_id: None,
        step: None,
        ts: "2000-01-01T00:00:00Z".into(),
        event: Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "question".repeat(64),
            attachments: vec![],
        },
    }];
    let f = Fixture::with_history(vec![answer()], history).await?;
    f.gateway.set_bound(true);
    let mut deps = f.deps()?;
    deps.limits.history_bytes = 64;
    deps.limits.history_page_bytes = 64;
    let runtime = Runtime::new(deps)?;
    assert_eq!(
        runtime
            .run(tokio_util::sync::CancellationToken::new())
            .await,
        Err(RuntimeError::Limit)
    );
    assert!(f.gateway.model_requests().is_empty());
    assert_eq!(RuntimeError::Limit.code(), ErrorCode::Budget);
    f.gateway.shutdown().await?;
    Ok(())
}
