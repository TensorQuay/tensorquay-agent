//! CORE-U-062: a model attempt that failed because the runtime's own link to the gateway was down is not spent. The
//! runner asks the link to settle (a proven round trip, or a wait for it to come back, within a per-step budget); a
//! restored link gives the attempt back, and a healthy or still-down link counts it exactly as before.
mod support;
use std::{
    error::Error,
    sync::{Arc, Mutex},
    time::Duration,
};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{GatewayLink, LinkSettle, RouteProvider, RunOutcome, Runner};
use tqa_llm::{ConnectionFailure, LlmError, Registry, ThrottleScope};
use tqa_testkit::{FakeProvider, ProviderReply};
use tqa_tools::ToolRegistry;
use tqa_types::{ErrorCode, Event, RouteSnapshot};

const BUDGET: Duration = Duration::from_mins(2);

/// A link that is down for `outage` each time it is asked (restored if the budget covers it), or always healthy.
struct FakeLink {
    outage: Option<Duration>,
    budgets: Mutex<Vec<Duration>>,
    hang: bool,
}
impl FakeLink {
    fn new(outage: Option<Duration>) -> Arc<Self> {
        Arc::new(Self {
            outage,
            budgets: Mutex::new(Vec::new()),
            hang: false,
        })
    }
    fn budgets(&self) -> Vec<Duration> {
        self.budgets.lock().map(|b| b.clone()).unwrap_or_default()
    }
}
#[async_trait::async_trait]
impl GatewayLink for FakeLink {
    fn budget(&self) -> Duration {
        BUDGET
    }
    async fn settle(&self, budget: Duration) -> LinkSettle {
        if let Ok(mut budgets) = self.budgets.lock() {
            budgets.push(budget);
        }
        if self.hang {
            std::future::pending::<()>().await;
        }
        match self.outage {
            None => LinkSettle::Reachable,
            Some(outage) if outage <= budget => LinkSettle::Restored(outage),
            Some(_) => LinkSettle::Unreachable(budget),
        }
    }
}

fn failing(error: fn() -> LlmError) -> ProviderReply {
    ProviderReply {
        delay: Duration::ZERO,
        result: Err(error()),
    }
}
fn cut() -> LlmError {
    LlmError::StreamCut {
        partial: Box::default(),
    }
}
fn refused() -> LlmError {
    LlmError::Connection(ConnectionFailure::Refused)
}

async fn run_with(
    script: Vec<ProviderReply>,
    link: Option<Arc<FakeLink>>,
    cancel: CancellationToken,
) -> Result<(RunOutcome, Vec<Event>), Box<dyn Error>> {
    let mut h = Harness::new(script, ToolRegistry::new())?;
    h.deps.gateway_link = link.map(|link| link as Arc<dyn GatewayLink>);
    let runner = Runner::new(h.deps)?;
    let outcome = drive(
        runner.run(&[], Some(turn()), cancel, CancellationToken::new()),
        &h.clock,
    )
    .await??;
    Ok((outcome, h.sink.events()))
}

/// ("step", attempt) and ("retry", attempt, reason) in order.
fn trail(events: &[Event]) -> Vec<(String, u32, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::StepStarted { attempt, .. } => Some(("step".into(), *attempt, String::new())),
            Event::ProviderRetry {
                attempt, reason, ..
            } => Some(("retry".into(), *attempt, reason.clone())),
            _ => None,
        })
        .collect()
}

fn link_retry_delays(events: &[Event]) -> Vec<u64> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry {
                delay_ms, reason, ..
            } if reason == "gateway_link" => Some(*delay_ms),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn core_u_062_a_restored_link_gives_the_attempt_back_and_reports_the_wait()
-> Result<(), Box<dyn Error>> {
    for error in [cut as fn() -> LlmError, refused] {
        let link = FakeLink::new(Some(Duration::from_secs(45)));
        // Three failures would exhaust the attempts; the first is the link's, so the fourth reply still runs.
        let script = vec![failing(error), failing(error), failing(error), answer()];
        let (outcome, events) =
            run_with(script, Some(link.clone()), CancellationToken::new()).await?;
        assert_eq!(outcome, RunOutcome::Completed);
        let trail = trail(&events);
        // The link's retry names the failed attempt, then numbers move on: unique and increasing.
        assert_eq!(trail[0], ("step".into(), 1, String::new()));
        assert_eq!(trail[1], ("retry".into(), 1, "gateway_link".into()));
        assert_eq!(trail[2], ("step".into(), 2, String::new()));
        let attempts: Vec<u32> = trail
            .iter()
            .filter(|t| t.0 == "step")
            .map(|t| t.1)
            .collect();
        assert_eq!(attempts, [1, 2, 3, 4]);
        // Reported after the wait, with the time actually waited.
        assert_eq!(link_retry_delays(&events)[0], 45_000);
        assert_eq!(link.budgets()[0], BUDGET);
    }
    Ok(())
}

#[tokio::test]
async fn core_u_062_a_healthy_link_counts_a_genuine_failure_exactly_as_before()
-> Result<(), Box<dyn Error>> {
    let link = FakeLink::new(None);
    let script = vec![failing(cut), failing(cut), failing(cut)];
    let (outcome, events) = run_with(script, Some(link.clone()), CancellationToken::new()).await?;
    assert_eq!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::ProviderError,
            reason: "stream_cut".into()
        }
    );
    assert!(link_retry_delays(&events).is_empty());
    assert_eq!(
        link.budgets().len(),
        3,
        "each transport failure asks the link"
    );
    let steps = trail(&events).iter().filter(|t| t.0 == "step").count();
    assert_eq!(steps, 3, "three attempts, as without a link");
    Ok(())
}

#[tokio::test]
async fn core_u_062_a_flapping_link_spends_the_steps_budget_then_failures_count()
-> Result<(), Box<dyn Error>> {
    // Each outage is 70 s: the first fits the 120 s budget (given back), the second finds only 50 s left (counted),
    // and afterwards the budget is spent, so the link is not asked again and failures count as before.
    let link = FakeLink::new(Some(Duration::from_secs(70)));
    let script = vec![failing(cut), failing(cut), failing(cut), failing(cut)];
    let (outcome, events) = run_with(script, Some(link.clone()), CancellationToken::new()).await?;
    assert_eq!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::ProviderError,
            reason: "stream_cut".into()
        }
    );
    assert_eq!(link.budgets(), [BUDGET, Duration::from_secs(50)]);
    assert_eq!(link_retry_delays(&events), [70_000]);
    let steps = trail(&events).iter().filter(|t| t.0 == "step").count();
    assert_eq!(steps, 4, "one attempt given back, then the usual three");
    Ok(())
}

#[tokio::test]
async fn core_u_062_other_failures_never_ask_the_link() -> Result<(), Box<dyn Error>> {
    let throttled = || LlmError::Throttled {
        retry_after: Duration::from_secs(1),
        scope: ThrottleScope::Route,
    };
    let unavailable = || LlmError::Http {
        status: 503,
        body_excerpt: String::new(),
    };
    for script in [
        vec![failing(throttled), answer()],
        vec![failing(unavailable), answer()],
    ] {
        let link = FakeLink::new(Some(Duration::from_secs(5)));
        let (outcome, _) = run_with(script, Some(link.clone()), CancellationToken::new()).await?;
        assert_eq!(outcome, RunOutcome::Completed);
        assert!(
            link.budgets().is_empty(),
            "the link is consulted only for transport failures"
        );
    }
    // An empty reply is not a transport failure either.
    let link = FakeLink::new(Some(Duration::from_secs(5)));
    let empty = reply(vec![
        usage(),
        tqa_llm::StreamChunk::Finish(tqa_llm::FinishReason::Stop),
    ]);
    let (_, _) = run_with(
        vec![empty, answer()],
        Some(link.clone()),
        CancellationToken::new(),
    )
    .await?;
    assert!(link.budgets().is_empty());
    Ok(())
}

#[tokio::test]
async fn core_u_062_cancelling_while_the_link_settles_cancels_the_run() -> Result<(), Box<dyn Error>>
{
    let link = Arc::new(FakeLink {
        outage: None,
        budgets: Mutex::new(Vec::new()),
        hang: true,
    });
    let cancel = CancellationToken::new();
    let canceller = cancel.clone();
    tokio::spawn(async move {
        tokio::task::yield_now().await;
        canceller.cancel();
    });
    let (outcome, events) = run_with(vec![failing(cut), answer()], Some(link), cancel).await?;
    assert_eq!(outcome, RunOutcome::Cancelled);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::RunFailed { .. })),
        "never a provider failure"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_062_without_a_link_nothing_changes() -> Result<(), Box<dyn Error>> {
    let (outcome, events) = run_with(
        vec![failing(cut), failing(cut), failing(cut)],
        None,
        CancellationToken::new(),
    )
    .await?;
    assert_eq!(
        outcome,
        RunOutcome::Failed {
            code: ErrorCode::ProviderError,
            reason: "stream_cut".into()
        }
    );
    assert!(link_retry_delays(&events).is_empty());
    Ok(())
}

/// A declared fallback route whose provider records whether it was asked.
fn with_backup(h: &mut Harness) -> Result<Arc<FakeProvider>, Box<dyn Error>> {
    let routes = Registry::from_toml(
        "[[routes]]\nname='backup'\nprotocol='openai_chat'\nbase_url='http://localhost'\nmodel='backup-model'\nauth={kind='none'}\n",
    )?;
    let provider = Arc::new(FakeProvider::new(
        routes.get("backup").ok_or("missing route")?.clone(),
        vec![answer()],
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

#[tokio::test]
async fn core_u_062_a_link_outage_never_switches_to_a_fallback_route() -> Result<(), Box<dyn Error>>
{
    // A refused connection is switchable: with a healthy link, three of them exhaust the primary and the run moves to
    // the backup route (the control). With the link down and restored each time, every attempt is given back before
    // the switch is considered, so the run stays on its own route and never spends the fallback.
    for (outage, switches) in [(None, true), (Some(Duration::from_secs(10)), false)] {
        // Three refusals exhaust the primary's attempts, which is when a switchable failure moves the run on.
        let script = vec![
            failing(refused),
            failing(refused),
            failing(refused),
            answer(),
        ];
        let mut h = Harness::new(script, ToolRegistry::new())?;
        let backup = with_backup(&mut h)?;
        h.deps.gateway_link = Some(FakeLink::new(outage) as Arc<dyn GatewayLink>);
        let clock = h.clock.clone();
        let sink = h.sink.clone();
        let outcome = drive(
            Runner::new(h.deps)?.run(
                &[],
                Some(turn()),
                CancellationToken::new(),
                CancellationToken::new(),
            ),
            &clock,
        )
        .await??;
        assert_eq!(outcome, RunOutcome::Completed, "outage {outage:?}");
        let switched = sink
            .events()
            .iter()
            .any(|event| matches!(event, Event::RouteFallback { .. }));
        assert_eq!(switched, switches, "outage {outage:?}");
        assert_eq!(backup.requests().is_empty(), !switches, "outage {outage:?}");
    }
    Ok(())
}
