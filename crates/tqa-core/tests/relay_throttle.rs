//! CORE-U-046 / CORE-U-047 / CORE-U-048: the relay's own throttle is waited out, not spent as provider failures.
mod support;
use std::{error::Error, sync::Arc, time::Duration};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RouteProvider, RunOutcome, Runner};
use tqa_llm::{LlmError, Registry, ThrottleScope};
use tqa_testkit::{FakeProvider, ProviderReply};
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event, RouteSnapshot};

fn throttled(scope: ThrottleScope, seconds: u64) -> ProviderReply {
    ProviderReply {
        delay: Duration::ZERO,
        result: Err(LlmError::Throttled {
            retry_after: Duration::from_secs(seconds),
            scope,
        }),
    }
}
fn throttles(scope: ThrottleScope, seconds: u64, count: usize) -> Vec<ProviderReply> {
    (0..count).map(|_| throttled(scope, seconds)).collect()
}
fn retries(events: &[Event]) -> Vec<(u64, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry {
                delay_ms, reason, ..
            } => Some((*delay_ms, reason.clone())),
            _ => None,
        })
        .collect()
}
fn switches(events: &[Event]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::RouteFallback { to, reason, .. } => Some((to.clone(), reason.clone())),
            _ => None,
        })
        .collect()
}
fn with_backup(
    h: &mut Harness,
    script: Vec<ProviderReply>,
) -> Result<Arc<FakeProvider>, Box<dyn Error>> {
    let routes = Registry::from_toml(
        "[[routes]]\nname='backup'\nprotocol='openai_chat'\nbase_url='http://localhost'\nmodel='backup-model'\nauth={kind='none'}\n",
    )?;
    let provider = Arc::new(FakeProvider::new(
        routes.get("backup").ok_or("missing route")?.clone(),
        script,
        Arc::new(h.sleeper.clone()),
    ));
    h.deps.fallback_routes = vec![RouteProvider {
        route: RouteSnapshot {
            name: "backup".into(),
            relay_path: "/relay/r/backup".into(),
            model: "backup-model".into(),
            ..route()
        },
        provider: provider.clone(),
    }];
    Ok(provider)
}
async fn run(h: Harness) -> Result<RunOutcome, Box<dyn Error>> {
    let clock = h.clock.clone();
    Ok(drive(
        Runner::new(h.deps)?.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &clock,
    )
    .await??)
}

#[tokio::test]
async fn core_u_046_a_throttled_run_waits_and_completes_without_spending_attempts() -> TestResult {
    // Ten throttles are far more than the three-attempt budget; each is waited out.
    let mut script = throttles(ThrottleScope::User, 1, 10);
    script.push(answer());
    let h = Harness::new(script, ToolRegistry::new())?;
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(
        provider.requests().len(),
        11,
        "every request was made and metered"
    );
    // One retry event for the unchanged delay, not one per throttle.
    assert_eq!(retries(&sink.events()), vec![(1000, "rate_limited".into())]);
    // A provider failure after throttles still has its whole budget.
    let mut script = throttles(ThrottleScope::User, 1, 4);
    script.extend([http(503), http(503), answer()]);
    let h = Harness::new(script, ToolRegistry::new())?;
    assert_eq!(run(h).await?, RunOutcome::Completed);
    Ok(())
}

#[tokio::test]
async fn core_u_046_retry_events_coalesce_by_delay_and_every_thirty_seconds() -> TestResult {
    let mut script = throttles(ThrottleScope::User, 1, 2);
    script.extend(throttles(ThrottleScope::User, 2, 2));
    script.push(answer());
    let h = Harness::new(script, ToolRegistry::new())?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(
        retries(&sink.events()),
        vec![(1000, "rate_limited".into()), (2000, "rate_limited".into())]
    );
    // An unchanged delay is reported again once 30 seconds have passed since the last report.
    let mut script = throttles(ThrottleScope::User, 1, 40);
    script.push(answer());
    let h = Harness::new(script, ToolRegistry::new())?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(retries(&sink.events()).len(), 2);
    Ok(())
}

#[tokio::test]
async fn core_u_047_only_a_route_throttle_moves_the_run_to_a_fallback() -> TestResult {
    // The user's rate is shared by every route, so switching cannot help.
    let mut script = throttles(ThrottleScope::User, 1, 5);
    script.push(answer());
    let mut h = Harness::new(script, ToolRegistry::new())?;
    let backup = with_backup(&mut h, vec![answer()])?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert!(backup.requests().is_empty());
    assert!(switches(&sink.events()).is_empty());
    // A route throttle asking for more than a minute switches at once (ADR-0025).
    let mut h = Harness::new(
        vec![throttled(ThrottleScope::Route, 90)],
        ToolRegistry::new(),
    )?;
    let backup = with_backup(&mut h, vec![answer()])?;
    let sink = h.sink.clone();
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(backup.requests().len(), 1);
    assert_eq!(
        switches(&sink.events()),
        vec![("backup".into(), "rate_limited".into())]
    );
    // Short route throttles switch once they have lasted a retry budget.
    let mut h = Harness::new(throttles(ThrottleScope::Route, 1, 3), ToolRegistry::new())?;
    let backup = with_backup(&mut h, vec![answer()])?;
    let (primary, sink) = (h.provider.clone(), h.sink.clone());
    assert_eq!(run(h).await?, RunOutcome::Completed);
    assert_eq!(primary.requests().len(), 3);
    assert_eq!(backup.requests().len(), 1);
    assert_eq!(
        switches(&sink.events()),
        vec![("backup".into(), "rate_limited".into())]
    );
    Ok(())
}

#[tokio::test]
async fn core_u_048_only_the_wall_clock_ends_a_throttled_wait() -> TestResult {
    // With no fallback, neither scope ever fails as a provider error: the run waits until its wall clock ends it.
    for scope in [ThrottleScope::User, ThrottleScope::Route] {
        let mut h = Harness::new(throttles(scope, 1, 200), ToolRegistry::new())?;
        h.deps.limits.wall_clock_secs = 30;
        let provider = h.provider.clone();
        let outcome = run(h).await?;
        assert_eq!(
            outcome,
            RunOutcome::Failed {
                code: ErrorCode::Budget,
                reason: "budget.wall_clock".into(),
            },
            "{scope:?}"
        );
        // About one request a second for the whole 30 seconds, far past the three-attempt budget.
        let requests = provider.requests().len();
        assert!(
            (25..=31).contains(&requests),
            "{scope:?}: {requests} requests"
        );
    }
    Ok(())
}
