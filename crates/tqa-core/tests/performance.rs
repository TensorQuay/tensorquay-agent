//! PERF-001: ten-step loop overhead with zero-latency scripted model and tool replies.
#[path = "performance/journal.rs"]
mod journal;
#[path = "performance/report.rs"]
mod report;
#[path = "performance/run.rs"]
mod run;
mod support;

use support::TestResult;
use tokio::runtime::Builder;
use tqa_core::{EventContext, EventSink};
use tqa_types::{Event, Seq, ServerFrame};

const STEPS: u32 = 10;
const WARMUPS: usize = 5;
const SAMPLES: usize = 50;

#[test]
#[ignore = "PERF-001 requires release mode and an idle host; run explicitly"]
fn perf_001_harness_overhead() -> TestResult {
    if cfg!(debug_assertions) {
        return Err("PERF-001 must run with --release".into());
    }
    let runtime = Builder::new_current_thread().enable_all().build()?;
    let mut environment = report::environment()?;
    let memory = measure(&runtime, run::Variant::Memory)?;
    memory.print("memory")?;
    let journal = measure(&runtime, run::Variant::Journal)?;
    journal.print("journal")?;
    environment["load_after"] = std::fs::read_to_string("/proc/loadavg")
        .unwrap_or_default()
        .trim()
        .into();
    let failures = journal.failures();
    report::write(
        &environment,
        &serde_json::json!({"memory":memory.json(), "journal":journal.json()}),
        &failures,
    )?;
    failures
        .first()
        .map_or(Ok(()), |failure| Err(failure.clone().into()))
}

fn measure(
    runtime: &tokio::runtime::Runtime,
    variant: run::Variant,
) -> Result<report::Summary, Box<dyn std::error::Error>> {
    for _ in 0..WARMUPS {
        run::sample(runtime, variant)?;
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        samples.push(run::sample(runtime, variant)?.run_ns);
    }
    report::Summary::new(samples)
}

#[test]
fn perf_001_both_variants_complete_the_script() -> TestResult {
    let runtime = Builder::new_current_thread().enable_all().build()?;
    for variant in [run::Variant::Memory, run::Variant::Journal] {
        let sample = run::sample(&runtime, variant)?;
        assert!(sample.run_ns > 0);
        run::validate(&sample.frames)?;
    }
    Ok(())
}

#[test]
fn perf_001_incomplete_traces_cannot_be_samples() -> TestResult {
    let runtime = Builder::new_current_thread().enable_all().build()?;
    let sample = run::sample(&runtime, run::Variant::Memory)?;
    for event_type in ["step.completed", "run.completed", "tool.result"] {
        let mut frames = sample.frames.clone();
        let position = frames
            .iter()
            .position(|frame| match frame {
                ServerFrame::Event { event, .. } => {
                    serde_json::to_value(event).is_ok_and(|value| value["type"] == event_type)
                }
                _ => false,
            })
            .ok_or("missing test event")?;
        frames.remove(position);
        assert!(
            run::validate(&frames).is_err(),
            "accepted missing {event_type}"
        );
    }
    Ok(())
}

#[test]
fn perf_001_percentiles_use_whole_run_time_divided_by_ten() -> TestResult {
    let samples: Vec<_> = (1..=50_u64).rev().map(|ms| ms * 10 * 1_000_000).collect();
    let summary = report::Summary::new(samples)?;
    assert!((summary.p50_ms - 25.0).abs() < f64::EPSILON);
    assert!((summary.p99_ms - 50.0).abs() < f64::EPSILON);
    assert!(report::Summary::new(vec![]).is_err());
    assert!(report::Summary::new(vec![1; 49]).is_err());
    Ok(())
}

#[test]
fn perf_001_failed_tool_results_cannot_be_samples() -> TestResult {
    let runtime = Builder::new_current_thread().enable_all().build()?;
    let mut sample = run::sample(&runtime, run::Variant::Memory)?;
    let result = sample
        .frames
        .iter_mut()
        .find_map(|frame| match frame {
            ServerFrame::Event {
                event: Event::ToolResult { status, .. },
                ..
            } => Some(status),
            _ => None,
        })
        .ok_or("missing scripted tool result")?;
    *result = tqa_types::ToolStatus::Error;
    assert!(run::validate(&sample.frames).is_err());
    Ok(())
}

#[test]
fn perf_001_journal_syncs_at_step_end_run_end_and_one_hundred_records() -> TestResult {
    let runtime = Builder::new_current_thread().enable_all().build()?;
    let journal = journal::Journal::new(&runtime)?;
    let context = EventContext {
        run_id: Some("perf-run".parse()?),
        step: Some(1),
    };
    runtime.block_on(async {
        for number in 1..=202 {
            let event = match number {
                101 => Event::StepCompleted {
                    step: 1,
                    duration_ms: 0,
                },
                202 => Event::RunCompleted {
                    run_id: "perf-run".parse()?,
                },
                _ => Event::TextDelta {
                    text: "probe".into(),
                },
            };
            assert_eq!(
                journal.sink.emit(event, context.clone()).await?,
                Seq::from(number)
            );
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    let snapshot = journal.finish(&runtime)?;
    assert_eq!(snapshot.frames.len(), 202);
    assert_eq!(snapshot.synced, [100, 101, 201, 202].map(Seq::from));
    Ok(())
}
