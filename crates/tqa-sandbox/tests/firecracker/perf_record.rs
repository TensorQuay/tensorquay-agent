//! PERF-011's kept result (TEST-PLAN §15.1): the same schema and place as the stack harnesses' results
//! (`deploy/tests/perf_common.py`), so a run keeps its values, its images and the host's load beside them, and
//! `perf_common.py compare` reads it: the environment's `utc` and load have the Python side's forms, and the summary
//! holds only the millisecond values it compares.
#![allow(dead_code, reason = "Each test binary uses the parts it needs.")]
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// PRD M3: cold boot p50 at most 2 s.
pub const TARGET_P50_MS: u128 = 2000;

/// Nearest-rank percentiles over sorted samples, in milliseconds (`perf_common.percentile`); `None` without samples.
pub fn summary(sorted: &[u128]) -> Option<Value> {
    let rank = |percent: usize| {
        sorted
            .get((sorted.len() * percent).div_ceil(100).max(1) - 1)
            .copied()
    };
    Some(
        json!({"p50_ms": rank(50)?, "p99_ms": rank(99)?, "min_ms": sorted.first()?, "max_ms": sorted.last()?}),
    )
}

/// The whole result: the samples, their summary, the environment, the target and its verdict.
pub fn record(mut samples: Vec<u128>, environment: &Value, config: &Value) -> Option<Value> {
    samples.sort_unstable();
    let summary = summary(&samples)?;
    let passed = summary["p50_ms"]
        .as_u64()
        .is_some_and(|p50| u128::from(p50) <= TARGET_P50_MS);
    Some(
        json!({"id": "PERF-011", "environment": environment, "config": config, "samples": samples,
                "summary": summary, "target": {"p50_ms_at_most": TARGET_P50_MS},
                "verdict": if passed { "passed" } else { "failed" }}),
    )
}

/// The Python side's UTC form (`2026-09-29T06:15:02Z`) from the host record's compact one (`20260929T061502Z`).
pub fn extended_utc(compact: &str) -> Option<String> {
    let (date, time) = compact.strip_suffix('Z')?.split_once('T')?;
    if date.len() != 8
        || time.len() != 6
        || !(date.chars().chain(time.chars())).all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(format!(
        "{}-{}-{}T{}:{}:{}Z",
        &date[..4],
        &date[4..6],
        &date[6..],
        &time[..2],
        &time[2..4],
        &time[4..]
    ))
}

/// `tests/perf/results/<date>/perf-011-<HHMMSS>.json` under `root`, from an extended UTC stamp.
pub fn path(root: &Path, utc: &str) -> Option<PathBuf> {
    let (date, time) = utc.strip_suffix('Z')?.split_once('T')?;
    if date.len() != 10 || time.len() != 8 {
        return None;
    }
    let file = format!("perf-011-{}.json", time.replace(':', ""));
    Some(root.join("tests/perf/results").join(date).join(file))
}

/// The 1, 5 and 15 minute load averages from `/proc/loadavg`'s text, as `os.getloadavg()` gives them.
pub fn load_average(text: &str) -> Option<Value> {
    let fields: Vec<f64> = text
        .split_whitespace()
        .take(3)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (fields.len() == 3).then(|| json!(fields))
}

/// The host's IO pressure, or `unavailable` where the kernel has none (as `perf_common.io_pressure`).
pub fn io_pressure() -> String {
    std::fs::read_to_string("/proc/pressure/io")
        .map_or_else(|_| "unavailable".into(), |text| text.trim().into())
}
