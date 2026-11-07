//! RT-I-019: the runtime states its protocol revision and refuses a newer configuration once, cleanly.
#[path = "support/fixture.rs"]
mod support;
use std::time::Duration;
use support::*;
use tqa_types::{ErrorCode, PROTOCOL_REVISION, ServerFrame};

/// The gateway's default ready bound; the refusal must arrive well inside it.
const READY_BOUND: Duration = Duration::from_secs(30);

#[tokio::test]
async fn rt_i_019_the_link_upgrade_carries_the_protocol_revision() -> TestResult {
    let f = Fixture::new(vec![answer()]).await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    wait_for(|| f.gateway.registrations() == 1).await?;
    let links: Vec<_> = f
        .gateway
        .requests()
        .into_iter()
        .filter(|request| request.path == "/runtime/link")
        .collect();
    assert!(!links.is_empty());
    for link in links {
        assert_eq!(link.protocol_revision, Some(PROTOCOL_REVISION.to_string()));
    }
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn rt_i_019_a_newer_session_config_gets_one_error_then_a_close() -> TestResult {
    let f = Fixture::new(vec![answer()]).await?;
    let mut config = serde_json::to_value(session()?)?;
    config["field_from_a_later_revision"] = serde_json::json!({"value": "not echoed"});
    f.gateway.first_frame(serde_json::to_string(&config)?)?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    let started = std::time::Instant::now();
    let result =
        tokio::time::timeout(tqa_testkit::deadline::scaled(Duration::from_secs(5)), task).await;
    shutdown.cancel();
    let error = result??
        .err()
        .ok_or("a newer configuration must fail closed")?;
    assert_eq!(error.exit_code(), 2, "a configuration error is not retried");
    wait_for(|| f.gateway.closes() == 1).await?;
    assert!(started.elapsed() < READY_BOUND);
    let frames = f.gateway.frames();
    assert_eq!(frames.len(), 1, "{frames:?}");
    let ServerFrame::Error {
        target: None,
        seq: None,
        code: ErrorCode::UnsupportedProtocol,
        message,
        retryable: false,
    } = &frames[0]
    else {
        return Err(format!("expected one unsupported_protocol error: {frames:?}").into());
    };
    assert_eq!(
        message,
        &format!(
            "unsupported session.config field or value at $.field_from_a_later_revision \
             (runtime protocol revision {PROTOCOL_REVISION})"
        )
    );
    assert!(!message.contains("not echoed"));
    assert_eq!(f.gateway.registrations(), 1, "no redial after the refusal");
    assert!(f.gateway.stored().is_empty());
    f.gateway.shutdown().await?;
    Ok(())
}
