//! CORE-U-067 (GW-I-148's runtime half): when the gateway itself refuses a model call, the reason in `provider.retry`
//! and `run.failed` is `gateway.<code> <status>`, so the page never credits the provider with it, and no
//! `provider_message` is quoted (the body is the gateway's words). A provider's own failure keeps `http <status>`.
mod support;
use std::{error::Error, time::Duration};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_llm::LlmError;
use tqa_testkit::ProviderReply;
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event, ServerFrame};

fn refused(code: &str, status: u16) -> ProviderReply {
    ProviderReply {
        delay: Duration::ZERO,
        result: Err(LlmError::GatewayRefused {
            code: code.into(),
            status,
            retry_after: None,
        }),
    }
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

async fn run_once(h: Harness) -> Result<(RunOutcome, Vec<Event>), Box<dyn Error>> {
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
    Ok((outcome, events_of(&sink.frames())))
}

// TEST: CORE-U-067
#[tokio::test]
async fn core_u_067_a_gateway_refusal_is_retried_and_fails_as_the_gateways() -> TestResult {
    let h = Harness::new(
        vec![
            refused("key_use_unrecorded", 503),
            refused("key_use_unrecorded", 503),
            refused("key_use_unrecorded", 503),
        ],
        ToolRegistry::new(),
    )?;
    let (outcome, events) = run_once(h).await?;
    assert!(matches!(
        outcome,
        RunOutcome::Failed { code: ErrorCode::ProviderError, ref reason }
            if reason == "gateway.key_use_unrecorded 503"
    ));
    let retries: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry {
                reason,
                provider_message,
                ..
            } => Some((reason.clone(), provider_message.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        retries,
        vec![("gateway.key_use_unrecorded 503".to_owned(), None); 2]
    );
    let failed = events.iter().find_map(|event| match event {
        Event::RunFailed {
            code,
            reason,
            provider_message,
            ..
        } => Some((*code, reason.clone(), provider_message.clone())),
        _ => None,
    });
    assert_eq!(
        failed,
        Some((
            ErrorCode::ProviderError,
            "gateway.key_use_unrecorded 503".to_owned(),
            None
        ))
    );
    Ok(())
}

// TEST: CORE-U-067
#[tokio::test]
async fn core_u_067_a_refusal_that_isnt_retried_fails_at_once() -> TestResult {
    let h = Harness::new(vec![refused("forbidden", 403)], ToolRegistry::new())?;
    let (outcome, events) = run_once(h).await?;
    assert!(matches!(
        outcome,
        RunOutcome::Failed { ref reason, .. } if reason == "gateway.forbidden 403"
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::ProviderRetry { .. }))
    );
    Ok(())
}

// TEST: CORE-U-067
#[tokio::test]
async fn core_u_067_a_retry_after_a_refusal_completes() -> TestResult {
    let h = Harness::new(
        vec![refused("secret_unavailable", 503), answer()],
        ToolRegistry::new(),
    )?;
    let (outcome, events) = run_once(h).await?;
    assert_eq!(outcome, RunOutcome::Completed);
    assert!(events.iter().any(|event| matches!(
        event,
        Event::ProviderRetry { reason, provider_message: None, .. } if reason == "gateway.secret_unavailable 503"
    )));
    Ok(())
}

// TEST: CORE-U-079
/// A helper stopped at its team's helpers' share of the budget (GW-I-304's `team_budget_helpers`) ends its run as a
/// budget stop too, final, with the gateway's code in its reason, so its page can say why (real-world use before 17.0).
#[tokio::test]
async fn core_u_079_a_helpers_share_refusal_fails_the_run_as_budget_too() -> TestResult {
    let h = Harness::new(
        vec![refused("team_budget_helpers", 409)],
        ToolRegistry::new(),
    )?;
    let (outcome, events) = run_once(h).await?;
    assert!(
        matches!(
            outcome,
            RunOutcome::Failed { code: ErrorCode::Budget, ref reason } if reason == "gateway.team_budget_helpers 409"
        ),
        "{outcome:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::ProviderRetry { .. })),
        "never retried"
    );
    Ok(())
}

// TEST: CORE-U-079
/// An agent team's spent budget (ADR-0034 point 8) is final: the gateway's 409 `team_budget` isn't retried, and the run
/// fails with the code `budget`, as a run past its own limits does, not as a provider's failure.
#[tokio::test]
async fn core_u_079_a_team_budget_refusal_fails_the_run_as_budget_without_retrying() -> TestResult {
    let h = Harness::new(vec![refused("team_budget", 409)], ToolRegistry::new())?;
    let (outcome, events) = run_once(h).await?;
    assert!(
        matches!(
            outcome,
            RunOutcome::Failed { code: ErrorCode::Budget, ref reason } if reason == "gateway.team_budget 409"
        ),
        "{outcome:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::ProviderRetry { .. })),
        "never retried"
    );
    Ok(())
}
