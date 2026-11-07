//! CORE-U-052: the provider's own words, sanitised, reach `run.failed` and `provider.retry` as `provider_message`.
//! `reason` and `code` stay the bounded classification; non-HTTP failures carry none; the journal and a
//! later run over it hold the same text.
mod support;
use std::{error::Error, sync::Arc, time::Duration};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RouteProvider, RunOutcome, Runner, UserTurn};
use tqa_llm::{LlmError, Registry, TimeoutKind};
use tqa_testkit::{FakeProvider, ProviderReply};
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event, RouteSnapshot, ServerFrame};

fn http_body(status: u16, body: &str) -> ProviderReply {
    ProviderReply {
        delay: Duration::ZERO,
        result: Err(LlmError::Http {
            status,
            body_excerpt: body.into(),
        }),
    }
}
fn failure(events: &[Event]) -> Option<(ErrorCode, String, Option<String>)> {
    events.iter().find_map(|event| match event {
        Event::RunFailed {
            code,
            reason,
            provider_message,
            ..
        } => Some((*code, reason.clone(), provider_message.clone())),
        _ => None,
    })
}
fn retries(events: &[Event]) -> Vec<(String, Option<String>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry {
                reason,
                provider_message,
                ..
            } => Some((reason.clone(), provider_message.clone())),
            _ => None,
        })
        .collect()
}
fn events_of(frames: &[ServerFrame]) -> Vec<Event> {
    frames
        .iter()
        .filter_map(|frame| match frame {
            ServerFrame::Event { event, .. } => Some(event.clone()),
            _ => None,
        })
        .collect()
}
async fn run_once(h: Harness) -> Result<(RunOutcome, Vec<ServerFrame>), Box<dyn Error>> {
    let sink = h.sink.clone();
    let runner = Runner::new(h.deps)?;
    let outcome = drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    Ok((outcome, sink.frames()))
}

#[tokio::test]
async fn core_u_052_a_rejected_request_fails_with_the_providers_words() -> TestResult {
    let body = r#"{"error":{"message":"max_tokens is too large for Bearer abc123","type":"invalid_request_error"}}"#;
    let h = Harness::new(vec![http_body(400, body)], ToolRegistry::new())?;
    let (outcome, frames) = run_once(h).await?;
    assert!(matches!(
        outcome,
        RunOutcome::Failed { code: ErrorCode::ProviderError, ref reason } if reason == "http 400"
    ));
    let events = events_of(&frames);
    assert_eq!(
        failure(&events),
        Some((
            ErrorCode::ProviderError,
            "http 400".into(),
            Some("max_tokens is too large for Bearer [redacted]".into())
        )),
        "reason and code unchanged; the message sanitised"
    );
    let text = serde_json::to_string(&frames)?;
    assert!(
        !text.contains("abc123"),
        "the credential never reaches the journal"
    );
    assert_closed(&frames)?;
    Ok(())
}

#[tokio::test]
async fn core_u_052_each_retry_says_why_and_the_final_failure_keeps_the_last_words() -> TestResult {
    let h = Harness::new(
        vec![
            http_body(503, "overloaded, attempt one"),
            http_body(503, r#"{"message":"overloaded, attempt two"}"#),
            http_body(503, "overloaded, attempt three"),
        ],
        ToolRegistry::new(),
    )?;
    let (outcome, frames) = run_once(h).await?;
    assert!(matches!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::ProviderError,
            ..
        }
    ));
    let events = events_of(&frames);
    assert_eq!(
        retries(&events),
        vec![
            ("http 503".into(), Some("overloaded, attempt one".into())),
            ("http 503".into(), Some("overloaded, attempt two".into())),
        ]
    );
    assert_eq!(
        failure(&events)
            .and_then(|(_, _, message)| message)
            .as_deref(),
        Some("overloaded, attempt three")
    );
    Ok(())
}

#[tokio::test]
async fn core_u_052_a_retry_then_success_completes_with_the_message_on_the_retry() -> TestResult {
    let h = Harness::new(
        vec![http_body(500, "internal hiccup"), answer()],
        ToolRegistry::new(),
    )?;
    let (outcome, frames) = run_once(h).await?;
    assert_eq!(outcome, RunOutcome::Completed);
    let events = events_of(&frames);
    assert_eq!(
        retries(&events),
        vec![("http 500".into(), Some("internal hiccup".into()))]
    );
    assert_eq!(failure(&events), None);
    Ok(())
}

#[tokio::test]
async fn core_u_052_non_http_failures_carry_no_message() -> TestResult {
    let timeout = || ProviderReply {
        delay: Duration::ZERO,
        result: Err(LlmError::Timeout(TimeoutKind::Idle)),
    };
    let h = Harness::new(vec![timeout(), timeout(), timeout()], ToolRegistry::new())?;
    let (outcome, frames) = run_once(h).await?;
    assert!(matches!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::ProviderError,
            ..
        }
    ));
    let events = events_of(&frames);
    let retried = retries(&events);
    assert!(!retried.is_empty(), "the timeout was retried");
    assert!(retried.iter().all(|(_, message)| message.is_none()));
    assert_eq!(failure(&events).and_then(|(_, _, message)| message), None);
    // An empty body gives no message either: absent, never an empty string.
    let h = Harness::new(vec![http_body(400, " \n ")], ToolRegistry::new())?;
    let (_, frames) = run_once(h).await?;
    assert!(!serde_json::to_string(&frames)?.contains("provider_message"));
    Ok(())
}

#[tokio::test]
async fn core_u_052_a_switched_route_reports_each_providers_own_words() -> TestResult {
    let mut h = Harness::new(
        vec![
            http_body(503, "primary busy"),
            http_body(503, "primary busy"),
            http_body(503, "primary busy"),
        ],
        ToolRegistry::new(),
    )?;
    let routes = Registry::from_toml(
        "[[routes]]\nname='backup'\nprotocol='openai_chat'\nbase_url='http://localhost'\nmodel='backup-model'\nauth={kind='none'}\n",
    )?;
    let backup = Arc::new(FakeProvider::new(
        routes.get("backup").ok_or("missing route")?.clone(),
        vec![http_body(400, r#"{"error":"backup refuses this model"}"#)],
        Arc::new(h.sleeper.clone()),
    ));
    h.deps.fallback_routes = vec![RouteProvider {
        route: RouteSnapshot {
            name: "backup".into(),
            relay_path: "/relay/r/backup".into(),
            model: "backup-model".into(),
            ..route()
        },
        provider: backup,
    }];
    let (outcome, frames) = run_once(h).await?;
    assert!(matches!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::ProviderError,
            ..
        }
    ));
    let events = events_of(&frames);
    assert!(
        retries(&events)
            .iter()
            .all(|(_, message)| message.as_deref() == Some("primary busy"))
    );
    assert_eq!(
        failure(&events),
        Some((
            ErrorCode::ProviderError,
            "http 400".into(),
            Some("backup refuses this model".into())
        ))
    );
    Ok(())
}

#[tokio::test]
async fn core_u_052_the_journal_and_a_later_run_hold_the_same_text() -> TestResult {
    let words = "The model `x` does not exist — ¿seguro? 🚀";
    let h = Harness::new(
        vec![
            http_body(404, &format!(r#"{{"error":{{"message":"{words}"}}}}"#)),
            answer(),
        ],
        ToolRegistry::new(),
    )?;
    let sink = h.sink.clone();
    let runner = Runner::new(h.deps)?;
    drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    let journal = sink.frames();
    // Every journalled frame survives the wire parser byte for byte.
    for frame in &journal {
        let bytes = serde_json::to_vec(frame)?;
        assert_eq!(&ServerFrame::parse(&bytes)?, frame);
    }
    let again = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "again".into(),
        attachments: vec![],
    };
    let outcome = drive(
        runner.run(
            &journal,
            Some(again),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    assert_eq!(outcome, RunOutcome::Completed);
    let replayed = sink.frames();
    assert_eq!(
        &replayed[..journal.len()],
        &journal[..],
        "history unchanged"
    );
    let events = events_of(&replayed);
    assert_eq!(
        failure(&events)
            .and_then(|(_, _, message)| message)
            .as_deref(),
        Some(words)
    );
    Ok(())
}
