//! GW-U-092, the c16d first hour's F2: the README's default small model on a CPU-only computer answered the check's
//! two requests correctly, but thinking at a few tokens a second took longer than the check's 120 s. A local route's
//! check is now bounded by progress: each answer's first byte within `[relay] local_check_timeout`, then no gap
//! longer than `[relay] idle_timeout`, within an outer cap of 10 minutes. It reports the model's speed, and below 15
//! tokens a second it passes with `slow_answers`.

use super::RelayConfig;
use super::probe::{Check, Observation, deadline, longest_deadline};
use super::probe_tests::{NONCE, TestResult, call, lines, probe_with, route, start_model};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tqa_llm::Usage;
use tqa_testkit::{Script, Scripted};

fn counted(completion: u64) -> Scripted {
    Scripted::Usage(Usage {
        prompt_tokens: 20,
        completion_tokens: completion,
        cached_tokens: None,
        reasoning_tokens: None,
    })
}
/// An answer that arrives a little at a time: `gaps` pauses of `gap` before `last`, then its usage.
fn slowly(gaps: usize, gap: Duration, last: Scripted, completion: u64) -> Script {
    let mut events = Vec::new();
    for _ in 0..gaps {
        events.push(Scripted::Delay(gap));
        events.push(Scripted::Reasoning("Thinking about it. ".into()));
    }
    events.extend([last, counted(completion)]);
    Script::new(events)
}
/// The check's three answers (the tool call, the round trip, the strict-lines step), each `gaps` pauses long.
fn answers(gaps: usize, gap: Duration, completion: u64) -> Vec<Script> {
    vec![
        slowly(gaps, gap, call(NONCE), completion),
        slowly(gaps, gap, Scripted::Text("ok".into()), completion),
        slowly(gaps, gap, lines(&format!("{NONCE}\n{NONCE}")), completion),
    ]
}
async fn check(
    scripts: Vec<Script>,
    config: &RelayConfig,
) -> Result<Observation, Box<dyn std::error::Error>> {
    let server = start_model(scripts).await?;
    // The fake model is on this computer, so its route is a local one.
    let local = route("openai_chat", server.base_url(), "")?;
    let observed = probe_with(&local, config, None, &CancellationToken::new()).await?;
    server.shutdown().await?;
    Ok(observed)
}
fn code(observed: &Observation) -> Option<&'static str> {
    observed.failure.as_ref().map(|failure| failure.code)
}

// TEST: GW-U-092
#[tokio::test]
async fn gw_u_092_a_local_model_that_keeps_answering_passes_past_its_first_byte_wait_and_says_it_is_slow()
-> TestResult {
    // Each answer streams for about 2.4 s, the whole check over 7 s: well past the 1 s that once bounded it all, with
    // no gap near the 2 s idle limit. 20 tokens an answer, at about 8 a second.
    let config = RelayConfig {
        local_check_timeout: 1,
        idle_timeout: 2,
        ..RelayConfig::default()
    };
    let observed = check(answers(4, Duration::from_millis(600), 20), &config).await?;
    assert!(observed.ok, "{observed:?}");
    assert!(observed.total_ms > 3_000, "{observed:?}");
    let speed = observed.output_tokens_per_second.ok_or("no speed")?;
    assert!((1..15).contains(&speed), "{speed} tokens a second");
    assert!(observed.warnings.contains(&"slow_answers"), "{observed:?}");
    Ok(())
}

// TEST: GW-U-092
#[tokio::test]
async fn gw_u_092_a_local_model_that_stops_answering_still_times_out() -> TestResult {
    // A gap longer than the idle limit ends the check, however long the outer cap.
    let config = RelayConfig {
        idle_timeout: 1,
        ..RelayConfig::default()
    };
    let observed = check(answers(1, Duration::from_secs(3), 20), &config).await?;
    assert_eq!(code(&observed), Some("timeout"), "{observed:?}");
    assert!(!observed.warnings.contains(&"slow_answers"));
    Ok(())
}

// TEST: GW-U-092
#[tokio::test]
async fn gw_u_092_a_local_model_out_of_time_after_its_round_trip_passes_slowly() -> TestResult {
    // The 7 Nov 2026 first hour: the README's CPU model passed the call and the round trip, then ran out of time in the
    // strict-lines step (5,078 tokens of thinking at 12 a second reached the outer cap). It works for tasks, slowly:
    // the check passes, says so, and says the line breaks went unverified. Out of time before its round trip, a model
    // still fails with `timeout` (above).
    let config = RelayConfig {
        idle_timeout: 1,
        ..RelayConfig::default()
    };
    let mut scripts = answers(0, Duration::ZERO, 64);
    scripts[2] = slowly(
        1,
        Duration::from_secs(3),
        lines(&format!("{NONCE}\n{NONCE}")),
        64,
    );
    let observed = check(scripts, &config).await?;
    assert!(observed.ok, "{observed:?}");
    assert_eq!(observed.round_trip, Check::Passed, "{observed:?}");
    assert_eq!(observed.strict_line_breaks, Check::NotRun, "{observed:?}");
    for warning in ["line_breaks_unverified", "slow_answers"] {
        assert!(observed.warnings.contains(&warning), "{observed:?}");
    }
    Ok(())
}

// TEST: GW-U-092
#[tokio::test]
async fn gw_u_092_a_fast_local_model_reports_its_speed_without_the_warning() -> TestResult {
    let observed = check(answers(0, Duration::ZERO, 64), &RelayConfig::default()).await?;
    assert!(observed.ok, "{observed:?}");
    assert!(
        observed
            .output_tokens_per_second
            .is_some_and(|speed| speed >= 15),
        "{observed:?}"
    );
    assert!(!observed.warnings.contains(&"slow_answers"), "{observed:?}");
    Ok(())
}

// TEST: GW-U-092
#[test]
fn gw_u_092_a_local_check_may_run_ten_minutes_and_a_hosted_one_a_minute() -> TestResult {
    let config = RelayConfig::default();
    let local = route("openai_chat", "http://127.0.0.1:11434/v1", "")?;
    // A hosted service with a key (a route without one counts as local, whatever its address).
    let mut hosted = route("openai_chat", "https://api.example.com/v1", "")?;
    hosted.auth = tqa_llm::Auth::Bearer;
    assert_eq!(deadline(&local, &config), Duration::from_mins(10));
    assert_eq!(deadline(&hosted, &config), Duration::from_mins(1));
    // A first-byte wait set past ten minutes still fits: the check never ends before its first byte could.
    let patient = RelayConfig {
        local_check_timeout: 600,
        ..RelayConfig::default()
    };
    assert_eq!(deadline(&local, &patient), Duration::from_mins(10));
    assert_eq!(longest_deadline(&config), Duration::from_mins(10));
    Ok(())
}
