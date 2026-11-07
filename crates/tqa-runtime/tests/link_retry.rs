//! A model call whose path to the gateway is cut does not spend a model attempt (CORE-U-062 in the runner): the runtime's
//! link settles the path, by a ping that must be answered within `io_timeout`, or by waiting up to `link_wait` for it
//! to register again. The fake gateway's path switch cuts the stream, refuses requests and redials, and silences the
//! registered link, as a network cut does.
#[path = "support/fixture.rs"]
mod support;
use std::time::Duration;
use support::*;
use tqa_runtime::RuntimeLimits;
use tqa_testkit::{Script, Scripted};
use tqa_types::{Event, ServerFrame};

type RuntimeTask = (
    tokio_util::sync::CancellationToken,
    tokio::task::JoinHandle<Result<(), tqa_runtime::RuntimeError>>,
);

/// The first answer sends a delta, then waits on the fake clock: it is mid-stream when the path goes down.
fn streaming() -> Script {
    Script::new(vec![
        Scripted::Text("partial".into()),
        Scripted::Delay(Duration::from_hours(1)),
        Scripted::Text("never".into()),
    ])
}

/// The events the runtime journaled, which reach the gateway only once its link is back.
fn journaled(f: &Fixture) -> Result<Vec<Event>, Box<dyn std::error::Error>> {
    let wal = std::fs::read_to_string(f.directory.path().join("state/events.jsonl"))?;
    let mut events = Vec::new();
    for line in wal.lines() {
        let record: serde_json::Value = serde_json::from_str(line)?;
        if let ServerFrame::Event { event, .. } = serde_json::from_value(record["frame"].clone())? {
            events.push(event);
        }
    }
    Ok(events)
}

/// (`reason`, `delay_ms`) of each `provider.retry`.
fn retries(events: &[Event]) -> Vec<(String, u64)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::ProviderRetry {
                reason, delay_ms, ..
            } => Some((reason.clone(), *delay_ms)),
            _ => None,
        })
        .collect()
}

fn failed(events: &[Event]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, Event::RunFailed { .. }))
}

/// What the next step may advance. A sleep of `io_timeout` guards real I/O in flight (a journal append, a link write or
/// dial): advancing past it would turn that I/O into a timeout the scenario never meant, so nothing moves while one is
/// pending, and it resolves by itself. The probe's wait for its pong lasts as long but is named (`PROBE_WAIT`): it is
/// advanced only while the test holds the path down (`link_may_fail`), where its expiry is the lost link the scenario
/// expects, and never while the path is up, where the pong must answer it. Otherwise only waits shorter than
/// `io_timeout` are advanced (reconnect backoffs, retry delays); the model request's guards (first byte 30 s, idle
/// 60 s) and the heartbeat pass only as those add up.
fn advanceable(sleeper: &tqa_testkit::FakeSleeper, link_may_fail: bool) -> Option<Duration> {
    let guard = RuntimeLimits::RECOMMENDED.io_timeout;
    let pending = sleeper.pending_durations();
    let probe = sleeper.pending_named(tqa_runtime::testing::PROBE_WAIT);
    let guards = pending
        .iter()
        .filter(|pending| **pending == guard)
        .count()
        .saturating_sub(usize::from(probe));
    if guards > 0 {
        return None;
    }
    if probe {
        return link_may_fail.then_some(guard);
    }
    pending
        .iter()
        .copied()
        .filter(|pending| *pending < guard)
        .min()
}

/// Advances fake time by one wait `advanceable` allows, once real I/O lets it, and lets the woken sleep resolve; returns
/// how far, or `None` once `done` holds. The runtime runs on this test's thread, so the check and the advance see one
/// consistent set of sleeps.
async fn step(
    f: &Fixture,
    link_may_fail: bool,
    done: &mut impl FnMut(&Fixture) -> Result<bool, Box<dyn std::error::Error>>,
) -> Result<Option<Duration>, Box<dyn std::error::Error>> {
    let mut finished = Ok(false);
    let mut next = None;
    wait_for(|| {
        finished = done(f);
        next = advanceable(&f.sleeper, link_may_fail);
        !matches!(finished, Ok(false)) || next.is_some()
    })
    .await?;
    if finished? {
        return Ok(None);
    }
    let next = next.ok_or("no wait to advance")?;
    let count = |f: &Fixture| {
        f.sleeper
            .pending_durations()
            .iter()
            .filter(|pending| **pending == next)
            .count()
    };
    let (waiting, requested) = (count(f), f.sleeper.durations().len());
    f.clock.advance(next)?;
    // The woken task runs only when this one yields: wait until the advanced sleep has resolved (one fewer of its
    // duration pending) or a new sleep was requested, so the next check never sees it again and advances twice.
    wait_for(|| count(f) < waiting || f.sleeper.durations().len() > requested).await?;
    Ok(Some(next))
}

/// Steps fake time until `done`, for at most `limit` of it; returns the fake time spent.
async fn step_until(
    f: &Fixture,
    limit: Duration,
    link_may_fail: bool,
    mut done: impl FnMut(&Fixture) -> Result<bool, Box<dyn std::error::Error>>,
) -> Result<Duration, Box<dyn std::error::Error>> {
    let mut spent = Duration::ZERO;
    while let Some(moved) = step(f, link_may_fail, &mut done).await? {
        spent += moved;
        if spent > limit {
            return Err(format!("not done after {spent:?} of fake time").into());
        }
    }
    Ok(spent)
}

// `advanceable` takes an `io_timeout`-long sleep for an I/O guard, so no deliberate wait in these scenarios may last
// that long: turn a limits change that breaks this into a clear failure, not a stepper that waits and times out.
#[test]
fn rt_i_033_no_deliberate_wait_lasts_as_long_as_an_io_guard() {
    let limits = RuntimeLimits::RECOMMENDED;
    // The fixture's reconnect jitter is 0.5 of the full-jitter span, so a backoff is at most half the cap.
    assert!(limits.reconnect_cap / 2 < limits.io_timeout);
    assert_ne!(limits.heartbeat_interval, limits.io_timeout);
    assert_ne!(limits.link_wait, limits.io_timeout);
}

/// A run mid-stream when the path goes down.
async fn cut_mid_stream(
    scripts: Vec<Script>,
) -> Result<(Fixture, RuntimeTask), Box<dyn std::error::Error>> {
    let f = Fixture::in_memory(scripts).await?;
    f.gateway.set_bound(true);
    let task = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("text.delta").await?;
    f.gateway.set_path_down(true);
    Ok((f, task))
}

// TEST: RT-I-033
#[tokio::test]
async fn rt_i_033_a_cut_path_that_comes_back_gives_the_attempt_back() -> TestResult {
    let (f, (shutdown, task)) = cut_mid_stream(vec![streaming(), answer()]).await?;
    // The probe goes unanswered and the link redials into the cut; the path stays down for 30 s of fake time.
    let cut = tqa_types::Clock::now(&f.clock);
    step_until(&f, Duration::from_mins(1), true, |f| {
        Ok(
            tqa_types::Clock::now(&f.clock).saturating_duration_since(cut)
                >= Duration::from_secs(30),
        )
    })
    .await?;
    assert_eq!(
        f.gateway.registrations(),
        1,
        "no redial got through the cut"
    );
    f.gateway.set_path_down(false);
    step_until(&f, Duration::from_mins(1), false, |f| {
        Ok(journaled(f)?
            .iter()
            .any(|e| matches!(e, Event::RunCompleted { .. })))
    })
    .await?;
    f.gateway.wait_for_registrations(2).await?;
    let events = journaled(&f)?;
    assert!(!failed(&events));
    let retries = retries(&events);
    assert_eq!(retries.len(), 1, "{retries:?}");
    let (reason, waited) = &retries[0];
    assert_eq!(reason, "gateway_link");
    // Reported after the wait, with the time waited: the path's outage, within the step's budget.
    assert!(
        (30_000..=120_000).contains(waited),
        "delay_ms {waited} is the time waited"
    );
    assert_eq!(
        f.gateway.model_requests().len(),
        2,
        "the given-back attempt ran"
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

// TEST: RT-I-033
#[tokio::test]
async fn rt_i_033_a_genuine_cut_on_a_healthy_link_counts_as_before() -> TestResult {
    // The provider's stream is cut with the path up: the link answers the probe, so the attempt counts.
    let cut = Script::new(vec![
        Scripted::Text("partial".into()),
        Scripted::CutAfter(1),
    ]);
    let f = Fixture::in_memory(vec![cut, answer()]).await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    step_until(&f, Duration::from_mins(1), false, |f| {
        Ok(journaled(f)?
            .iter()
            .any(|e| matches!(e, Event::RunCompleted { .. })))
    })
    .await?;
    let reasons: Vec<String> = retries(&journaled(&f)?).into_iter().map(|r| r.0).collect();
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert_ne!(
        reasons[0], "gateway_link",
        "a reachable gateway spends the attempt"
    );
    assert_eq!(
        f.gateway.registrations(),
        1,
        "the probe never broke the link"
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

// TEST: RT-I-033
#[tokio::test]
async fn rt_i_033_a_path_still_down_at_link_wait_counts_and_the_run_fails_as_before() -> TestResult
{
    let (f, (shutdown, task)) = cut_mid_stream(vec![streaming()]).await?;
    let spent = step_until(&f, Duration::from_mins(10), true, |f| {
        Ok(failed(&journaled(f)?))
    })
    .await?;
    assert!(
        spent >= RuntimeLimits::RECOMMENDED.link_wait,
        "the step waited its whole budget ({spent:?}) before counting"
    );
    let events = journaled(&f)?;
    assert!(
        retries(&events)
            .iter()
            .all(|(reason, _)| reason != "gateway_link"),
        "nothing given back: {:?}",
        retries(&events)
    );
    assert_eq!(f.gateway.registrations(), 1);
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}

// TEST: RT-I-033
#[tokio::test]
async fn rt_i_033_stopping_while_the_path_is_down_ends_the_wait() -> TestResult {
    let (f, (shutdown, task)) = cut_mid_stream(vec![streaming()]).await?;
    // Into the wait: the probe went unanswered, and the settle waits for the link to register again.
    step_until(&f, Duration::from_mins(1), true, |f| {
        Ok(f.sleeper.pending_named(tqa_runtime::testing::SETTLE_WAIT))
    })
    .await?;
    stop(shutdown, task).await?;
    assert!(
        !failed(&journaled(&f)?),
        "stopping is never a provider failure"
    );
    f.gateway.shutdown().await?;
    Ok(())
}

/// Every frame the runtime journaled, in order.
fn wal_frames(f: &Fixture) -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
    let wal = std::fs::read_to_string(f.directory.path().join("state/events.jsonl"))?;
    let mut frames = Vec::new();
    for line in wal.lines() {
        let record: serde_json::Value = serde_json::from_str(line)?;
        frames.push(serde_json::from_value(record["frame"].clone())?);
    }
    Ok(frames)
}

// TEST: RT-I-033 (a gateway slow to store still answers the probe; GW-I-127 is the gateway's half)
#[tokio::test]
async fn rt_i_033_a_gateway_slow_to_store_answers_the_probe_so_nothing_redials() -> TestResult {
    // The provider cuts its own stream while the gateway's store is stalled: the gateway still reads the link and
    // answers the probe's ping, so the attempt counts as a genuine failure, the link is never dropped, and every frame
    // reaches the store in order once it recovers.
    let cut = Script::new(vec![
        Scripted::Text("partial".into()),
        Scripted::CutAfter(1),
    ]);
    let f = Fixture::in_memory(vec![cut, answer()]).await?;
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    let before = f.gateway.stored().len();
    f.gateway.set_store_stalled(true);
    f.gateway.send(user()).await?;
    step_until(&f, Duration::from_mins(1), false, |f| {
        Ok(journaled(f)?
            .iter()
            .any(|e| matches!(e, Event::RunCompleted { .. })))
    })
    .await?;
    assert_eq!(
        f.gateway.stored().len(),
        before,
        "the store held everything meanwhile"
    );
    assert_eq!(
        f.gateway.registrations(),
        1,
        "the probe got its pong: no redial"
    );
    let reasons: Vec<String> = retries(&journaled(&f)?).into_iter().map(|r| r.0).collect();
    assert_eq!(reasons.len(), 1, "{reasons:?}");
    assert_ne!(
        reasons[0], "gateway_link",
        "a reachable gateway spends the attempt"
    );
    f.gateway.set_store_stalled(false);
    let expected = wal_frames(&f)?;
    wait_for(|| f.gateway.stored().len() >= expected.len()).await?;
    assert_eq!(
        f.gateway.stored(),
        expected,
        "in order, nothing lost or repeated"
    );
    stop(shutdown, task).await?;
    f.gateway.shutdown().await?;
    Ok(())
}
