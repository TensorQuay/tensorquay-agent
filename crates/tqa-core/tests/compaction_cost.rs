//! PERF-030: the cost of compacting a 100-turn transcript, in time and tokens (TEST-PLAN §15.1). The explicit
//! benchmark records the environment, five warm-ups and fifty samples; given an earlier result in
//! `TQA_PERF_BASELINE`, a regression of more than 10 % in time or tokens fails it (a paired A/B on one host).
#[path = "performance/host.rs"]
mod host;
mod support;
use serde_json::{Value, json};
use std::{io::Write, path::Path, time::Instant};
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_context::estimate;
use tqa_core::{RunOutcome, Runner, UserTurn};
use tqa_llm::{FinishReason, StreamChunk};
use tqa_testkit::ProviderReply;
use tqa_tools::ToolRegistry;
use tqa_types::{CompactionSource, CompactionSummary, Event, ServerFrame};

const TURNS: usize = 100;
const WARMUPS: usize = 5;
const SAMPLES: usize = 50;
/// A window the 100-turn transcript overflows, so the run compacts before its first step.
const WINDOW: u32 = 16_384;
const RESERVE: u32 = 1024;
/// A result of about 600 bytes per turn, distinct so dedupe (CTX-U-012) does not shrink the transcript.
const RESULT_BYTES: usize = 600;
/// The largest regression the benchmark accepts against its baseline.
const TOLERANCE: f64 = 1.10;

/// One measured run: the time to compact and take the first step, and the token counts around the compaction.
struct Sample {
    run_ns: u64,
    summary_tokens: u64,
    step_tokens: u64,
}

fn transcript() -> Result<Vec<ServerFrame>, Box<dyn std::error::Error>> {
    let mut events = Vec::new();
    for turn in 1..=TURNS {
        let run_id: tqa_types::RunId = format!("r-{turn}").parse()?;
        let call_id: tqa_types::ToolCallId = format!("c{turn}").parse()?;
        events.extend([
            Event::MessageUser {
                lead_brief: None,
                team_wake: None,
                text: format!("request {turn}"),
                attachments: vec![],
            },
            Event::RunStarted {
                run_id: run_id.clone(),
            },
            Event::StepStarted {
                step: 1,
                attempt: 1,
            },
            Event::ToolCall {
                call_id: call_id.clone(),
                name: "read_file".into(),
                arguments: json!({"path": format!("src/{turn}.rs")}),
            },
            Event::ModelCompleted {
                step: 1,
                attempt: 1,
                finish_reason: tqa_types::FinishReason::ToolCalls,
            },
            Event::ToolResult {
                call_id,
                status: tqa_types::ToolStatus::Ok,
                content: format!("{turn:04}{}", "r".repeat(RESULT_BYTES - 4)),
                target: None,
                evidence: None,
            },
            Event::StepCompleted {
                step: 1,
                duration_ms: 0,
            },
            Event::StepStarted {
                step: 2,
                attempt: 1,
            },
            Event::TextDelta {
                text: format!("answer {turn}"),
            },
            Event::ModelCompleted {
                step: 2,
                attempt: 1,
                finish_reason: tqa_types::FinishReason::Stop,
            },
            Event::StepCompleted {
                step: 2,
                duration_ms: 0,
            },
            Event::RunCompleted { run_id },
        ]);
    }
    history(events)
}

fn text_reply(text: &str) -> ProviderReply {
    reply(vec![
        StreamChunk::TextDelta(text.into()),
        usage(),
        StreamChunk::Finish(FinishReason::Stop),
    ])
}

fn request_tokens(request: &tqa_llm::CompletionRequest) -> u64 {
    request
        .messages
        .iter()
        .map(|message| {
            let text: String = message
                .content
                .iter()
                .filter_map(|part| match part {
                    tqa_llm::ContentPart::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            u64::from(estimate(&text).tokens) + 4
        })
        .sum()
}

async fn sample(transcript: &[ServerFrame]) -> Result<Sample, Box<dyn std::error::Error>> {
    let summary = CompactionSummary {
        goal: "answer the requests".into(),
        done: vec!["read the sources".into()],
        pending: vec![],
        decisions: vec![],
        key_paths: vec!["src/".into()],
        open_errors: vec![],
    };
    let mut h = Harness::new(
        vec![text_reply(&serde_json::to_string(&summary)?), answer()],
        ToolRegistry::new(),
    )?;
    h.deps.route.context_window = Some(WINDOW);
    h.deps.route.max_output = Some(RESERVE);
    h.deps.limits.max_output_tokens = 1_000_000;
    h.sink_with(MemorySink::new(transcript));
    let (provider, sink) = (h.provider.clone(), h.sink.clone());
    let runner = Runner::new(h.deps)?;
    let turn = UserTurn {
        lead_brief: None,
        team_wake: None,
        text: "continue".into(),
        attachments: vec![],
    };
    let started = Instant::now();
    let outcome = runner
        .run(
            transcript,
            Some(turn),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    let run_ns = u64::try_from(started.elapsed().as_nanos())?;
    if outcome != RunOutcome::Completed {
        return Err(format!("PERF-030 run ended {outcome:?}").into());
    }
    let compactions = sink
        .events()
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::Compaction {
                    source: CompactionSource::Model,
                    ..
                }
            )
        })
        .count();
    let requests = provider.requests();
    if compactions != 1 || requests.len() != 2 || !requests[0].tools.is_empty() {
        return Err("PERF-030 requires exactly one model compaction before the step".into());
    }
    Ok(Sample {
        run_ns,
        summary_tokens: request_tokens(&requests[0]),
        step_tokens: request_tokens(&requests[1]),
    })
}

fn percentile(sorted: &[u64], percent: usize) -> f64 {
    // Nearest rank: with fifty samples p99 is the maximum.
    let rank = (sorted.len() * percent).div_ceil(100).max(1) - 1;
    f64::from(u32::try_from(sorted[rank] / 1000).unwrap_or(u32::MAX)) / 1000.0
}

#[tokio::test]
async fn perf_030_a_sample_compacts_once_and_counts_tokens() -> TestResult {
    let transcript = transcript()?;
    let first = sample(&transcript).await?;
    assert!(first.run_ns > 0 && first.summary_tokens > first.step_tokens && first.step_tokens > 0);
    let second = sample(&transcript).await?;
    assert_eq!(
        (first.summary_tokens, first.step_tokens),
        (second.summary_tokens, second.step_tokens),
        "tokens are deterministic"
    );
    Ok(())
}

#[test]
#[ignore = "PERF-030 needs release mode and an idle host; run explicitly"]
fn perf_030_compaction_cost_on_a_hundred_turn_transcript() -> TestResult {
    if cfg!(debug_assertions) {
        return Err("PERF-030 must run with --release".into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let transcript = transcript()?;
    let mut environment = host::environment()?;
    environment["load_before"] = host::load()?.into();
    for _ in 0..WARMUPS {
        runtime.block_on(sample(&transcript))?;
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        samples.push(runtime.block_on(sample(&transcript))?);
    }
    environment["load_after"] = host::load()?.into();
    let (summary_tokens, step_tokens) = (samples[0].summary_tokens, samples[0].step_tokens);
    let mut run_ns: Vec<u64> = samples.iter().map(|sample| sample.run_ns).collect();
    run_ns.sort_unstable();
    let result = json!({"benchmark": "PERF-030", "environment": environment, "turns": TURNS, "window": WINDOW,
        "warmups": WARMUPS, "samples": SAMPLES, "percentile": "nearest-rank",
        "timing": "monotonic whole Runner::run: planning, the summary call to a zero-latency fake model, projection and one step",
        "p50_ms": percentile(&run_ns, 50), "p99_ms": percentile(&run_ns, 99), "run_ns": run_ns,
        "summary_request_tokens": summary_tokens, "first_step_request_tokens": step_tokens});
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("missing workspace root")?;
    let timestamp = result["environment"]["utc"]
        .as_str()
        .ok_or("missing UTC timestamp")?;
    // A result is evidence: it is never overwritten, even by a run in the same second.
    let path = root.join(format!(
        "target/perf-results/perf-030-{timestamp}-{}.json",
        std::process::id()
    ));
    std::fs::create_dir_all(path.parent().ok_or("missing results directory")?)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(&serde_json::to_vec_pretty(&result)?)?;
    writeln!(
        std::io::stdout().lock(),
        "PERF-030 p50_ms={} p99_ms={} summary_tokens={summary_tokens} step_tokens={step_tokens} results={}",
        result["p50_ms"],
        result["p99_ms"],
        path.display()
    )?;
    if let Some(baseline) = std::env::var_os("TQA_PERF_BASELINE") {
        // Tests run from the crate directory; a relative baseline is taken from the workspace root, like results.
        let baseline: Value = serde_json::from_slice(&std::fs::read(root.join(baseline))?)?;
        for key in [
            "p50_ms",
            "summary_request_tokens",
            "first_step_request_tokens",
        ] {
            let (now, before) = (
                result[key].as_f64().ok_or(key)?,
                baseline[key].as_f64().ok_or(key)?,
            );
            assert!(
                now <= before * TOLERANCE,
                "PERF-030 {key} regressed more than 10 %: {now} against {before}"
            );
        }
    }
    Ok(())
}
