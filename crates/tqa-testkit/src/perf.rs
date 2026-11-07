//! The performance protocol's shared parts (TEST-PLAN §15.1): the environment a result was measured in, nearest-rank
//! percentiles, and the result file under `tests/perf/results/<date>/`. A result without its environment is not a
//! result: [`write_result`] refuses one. Raw data too large to sit inside a result goes beside it ([`write_raw`]),
//! named in the result with its SHA-256 so the result's own hash pins it.
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    fmt::Write as _,
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

/// The environment fields every result carries (PERF-001's record, the build profile and the load before the run).
pub const REQUIRED: &[&str] = &[
    "utc",
    "os",
    "kernel",
    "cpu",
    "logical_cores",
    "memory_bytes",
    "commit",
    "dirty_tree",
    "toolchain",
    "build",
    "load_before",
];

/// The nearest-rank `percent` percentile of `samples` (p99 of 50 samples is the maximum, as in PERF-001). Signed samples
/// are allowed: a difference between two paths can be negative within noise.
///
/// # Errors
/// No samples, or a percent outside 1 to 100.
pub fn percentile<T: Ord + Copy>(samples: &[T], percent: u32) -> Result<T, String> {
    if samples.is_empty() {
        return Err("no samples".into());
    }
    if !(1..=100).contains(&percent) {
        return Err(format!("percentile {percent} is outside 1 to 100"));
    }
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    let rank = (ordered.len() * percent as usize).div_ceil(100);
    ordered
        .get(rank.max(1) - 1)
        .copied()
        .ok_or_else(|| "rank out of range".into())
}

/// Checks that `environment` is an object holding every [`REQUIRED`] field.
///
/// # Errors
/// The first missing field, named.
pub fn validate_environment(environment: &Value) -> Result<(), String> {
    let fields = environment
        .as_object()
        .ok_or("the environment is not an object")?;
    match REQUIRED.iter().find(|key| !fields.contains_key(**key)) {
        Some(missing) => Err(format!(
            "the environment lacks {missing}: a result without its environment is not a result"
        )),
        None => Ok(()),
    }
}

/// Where a result measured at `utc` (`YYYY-MM-DDTHH:MM:SSZ`) is written under the repository `root`.
#[must_use]
pub fn result_path(root: &Path, id: &str, utc: &str) -> PathBuf {
    let date = utc.get(..10).unwrap_or("unknown-date");
    root.join("tests/perf/results")
        .join(date)
        .join(format!("{id}-{}.json", utc.replace(':', "-")))
}

/// Runs a metadata command with a fixed locale and returns its trimmed output.
fn command(program: &str, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(arguments)
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("performance metadata command failed: {program}"));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|_| format!("{program} printed something that isn't UTF-8"))
}

/// This host's environment record (Linux), with the build profile and the load average now.
///
/// # Errors
/// A metadata command that can't run.
pub fn environment() -> Result<Value, String> {
    let cpu = command("lscpu", &[])?
        .lines()
        .find_map(|line| {
            line.strip_prefix("Model name:")
                .map(|model| model.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".into());
    let pages: u64 = command("getconf", &["_PHYS_PAGES"])?
        .parse()
        .map_err(|_| "getconf _PHYS_PAGES")?;
    let page: u64 = command("getconf", &["PAGE_SIZE"])?
        .parse()
        .map_err(|_| "getconf PAGE_SIZE")?;
    Ok(json!({
        "utc": command("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"])?,
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "kernel": command("uname", &["-sr"])?, "cpu": cpu,
        "logical_cores": std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get),
        "memory_bytes": pages.saturating_mul(page),
        "commit": command("git", &["rev-parse", "HEAD"])?,
        "dirty_tree": !command("git", &["status", "--porcelain"])?.is_empty(),
        "toolchain": command("rustc", &["--version"])?,
        "build": if cfg!(debug_assertions) { "debug" } else { "release" },
        "load_before": std::fs::read_to_string("/proc/loadavg").map(|load| load.trim().to_owned()).unwrap_or_default(),
    }))
}

/// Writes `result` with its `environment` and verdict as `<root>/tests/perf/results/<date>/<id>-<utc>.json`, never over
/// an existing file, and returns the path. The verdict is `pass` with no `failures`, otherwise `fail` with them, so a
/// result file can stand as gate evidence on its own.
///
/// # Errors
/// An environment missing a [`REQUIRED`] field, or the file can't be written.
pub fn write_result(
    root: &Path,
    id: &str,
    environment: &Value,
    result: &Value,
    failures: &[String],
) -> Result<PathBuf, String> {
    validate_environment(environment)?;
    let utc = environment["utc"]
        .as_str()
        .ok_or("the environment's utc is not text")?;
    let path = result_path(root, id, utc);
    let directory = path.parent().ok_or("no results directory")?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    let verdict = if failures.is_empty() { "pass" } else { "fail" };
    let document = json!({"id": id, "environment": environment, "result": result, "verdict": verdict,
        "failures": failures});
    let bytes = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

/// Writes `raw` beside the result `<id>-<utc>.json` as `<id>-<utc>.<name>.json`, never over an existing file, and
/// returns the entry the result names it by: its file name, size and SHA-256.
///
/// # Errors
/// A `name` that isn't lowercase letters and hyphens, a `utc` that isn't text, or the file can't be written.
pub fn write_raw(
    root: &Path,
    id: &str,
    utc: &str,
    name: &str,
    raw: &Value,
) -> Result<Value, String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
    {
        return Err(format!(
            "raw data name {name:?}: lowercase letters and hyphens only"
        ));
    }
    let result = result_path(root, id, utc);
    let path = result.with_extension(format!("{name}.json"));
    let directory = path.parent().ok_or("no results directory")?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    let bytes = serde_json::to_vec(raw).map_err(|error| error.to_string())?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let sha256 = Sha256::digest(&bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    let file = path
        .file_name()
        .and_then(|file| file.to_str())
        .ok_or("raw file name")?;
    Ok(json!({"name": name, "file": file, "bytes": bytes.len(), "sha256": sha256}))
}

#[cfg(test)]
#[path = "perf_tests.rs"]
mod tests;
