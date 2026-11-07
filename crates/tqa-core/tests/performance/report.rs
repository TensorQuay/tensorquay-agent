//! Raw timings and the §15.1 environment, written through the shared performance kit; only the journal variant gates
//! PRD M2.
use super::{SAMPLES, STEPS, WARMUPS, support::TestResult};
use serde_json::{Value, json};
use std::{error::Error, io::Write, path::Path, time::Duration};

pub(super) struct Summary {
    pub p50_ms: f64,
    pub p99_ms: f64,
    run_ns: Vec<u64>,
}

impl Summary {
    pub(super) fn new(run_ns: Vec<u64>) -> Result<Self, Box<dyn Error>> {
        if run_ns.len() != SAMPLES {
            return Err("PERF-001 requires exactly fifty measured samples".into());
        }
        let mut sorted = run_ns.clone();
        sorted.sort_unstable();
        // Nearest-rank percentiles: p99 is the maximum with fifty samples.
        let percentile = |percent: usize| step_ms(sorted[(SAMPLES * percent).div_ceil(100) - 1]);
        Ok(Self {
            p50_ms: percentile(50),
            p99_ms: percentile(99),
            run_ns,
        })
    }

    pub(super) fn json(&self) -> Value {
        json!({"p50_ms":self.p50_ms, "p99_ms":self.p99_ms,
            "run_ns":self.run_ns, "per_step_ms":self.run_ns.iter().copied().map(step_ms).collect::<Vec<_>>()})
    }

    pub(super) fn print(&self, variant: &str) -> TestResult {
        Ok(writeln!(
            std::io::stdout().lock(),
            "PERF-001 {variant}: p50_ms={:.6} p99_ms={:.6} warmups={WARMUPS} samples={SAMPLES} steps={STEPS}",
            self.p50_ms,
            self.p99_ms
        )?)
    }

    /// The journal variant's misses against PRD M2 (≤ 50 ms p50, ≤ 200 ms p99), in words; none is a pass.
    pub(super) fn failures(&self) -> Vec<String> {
        let mut failures = Vec::new();
        if self.p50_ms > 50.0 {
            failures.push(format!(
                "PERF-001 journal p50 {:.6} ms exceeds 50 ms",
                self.p50_ms
            ));
        }
        if self.p99_ms > 200.0 {
            failures.push(format!(
                "PERF-001 journal p99 {:.6} ms exceeds 200 ms",
                self.p99_ms
            ));
        }
        failures
    }
}

fn step_ms(run_ns: u64) -> f64 {
    Duration::from_nanos(run_ns).as_secs_f64() * 1000.0 / f64::from(STEPS)
}

/// The §15.1 environment (the build profile and load included) with how this benchmark runs.
pub(super) fn environment() -> Result<Value, Box<dyn Error>> {
    let mut environment = tqa_testkit::perf::environment()?;
    environment["runtime"] = "current-thread tokio; owned blocking WAL writer".into();
    environment["sandbox"] = "not used; host harness benchmark".into();
    environment["image_digest"] = Value::Null;
    Ok(environment)
}

/// Writes the result under `tests/perf/results/<date>/` with its verdict, and prints where.
pub(super) fn write(environment: &Value, variants: &Value, failures: &[String]) -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("missing workspace root")?;
    let result = json!({"warmups":WARMUPS, "samples":SAMPLES, "steps":STEPS, "percentile":"nearest-rank",
        "timing":"monotonic whole Runner::run duration / 10; setup, validation and teardown excluded",
        "journal_sync":"each step.completed, each run end, or 100 records since the last sync",
        "variants":variants});
    let path = tqa_testkit::perf::write_result(root, "PERF-001", environment, &result, failures)?;
    Ok(writeln!(
        std::io::stdout().lock(),
        "PERF-001 environment={environment} results={}",
        path.display()
    )?)
}
