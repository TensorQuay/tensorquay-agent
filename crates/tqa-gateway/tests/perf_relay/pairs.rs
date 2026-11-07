//! PERF-003's accounting: pure, so its rules are tested without a gateway.
use serde_json::Value;

/// The target for the relay's added time per request, at p50 (TEST-PLAN PERF-003).
pub const ADDED_TOTAL_P50_MAX_MS: f64 = 10.0;
/// The target for the relay's added time to the first token, at p50.
pub const ADDED_FIRST_TOKEN_P50_MAX_MS: f64 = 20.0;

/// The two ways the same request reaches the model server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    Direct,
    Relay,
}

/// One streaming request's timing, from sending it: to its first content chunk, and to the end of the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub first_token_ns: i64,
    pub total_ns: i64,
}

/// Pair `index`'s order: the paths alternate which goes first, so a warm cache or a scheduler's favour doesn't always
/// land on the same one.
pub const fn order(index: usize) -> [Path; 2] {
    if index.is_multiple_of(2) {
        [Path::Direct, Path::Relay]
    } else {
        [Path::Relay, Path::Direct]
    }
}

/// Each pair's `(direct, relay)` difference, relay minus direct, as (total, first token) in nanoseconds.
pub fn added(pairs: &[(Timing, Timing)]) -> (Vec<i64>, Vec<i64>) {
    pairs
        .iter()
        .map(|(direct, relay)| {
            (
                relay.total_ns - direct.total_ns,
                relay.first_token_ns - direct.first_token_ns,
            )
        })
        .unzip()
}

/// The p50 and p99 of `added_ns`, in milliseconds (nearest rank).
pub fn summary(added_ns: &[i64]) -> Result<(f64, f64), String> {
    #[allow(
        clippy::cast_precision_loss,
        reason = "nanoseconds to milliseconds for a report"
    )]
    let ms = |ns: i64| ns as f64 / 1_000_000.0;
    Ok((
        ms(tqa_testkit::perf::percentile(added_ns, 50)?),
        ms(tqa_testkit::perf::percentile(added_ns, 99)?),
    ))
}

/// The gate: both p50s within their targets, or the first one missed, in plain words.
pub fn verdict(total_p50_ms: f64, first_token_p50_ms: f64) -> Result<(), String> {
    if total_p50_ms > ADDED_TOTAL_P50_MAX_MS {
        return Err(format!(
            "the relay adds {total_p50_ms:.3} ms at p50 per request (target ≤ {ADDED_TOTAL_P50_MAX_MS} ms)"
        ));
    }
    if first_token_p50_ms > ADDED_FIRST_TOKEN_P50_MAX_MS {
        return Err(format!(
            "the relay adds {first_token_p50_ms:.3} ms at p50 to the first token (target ≤ {ADDED_FIRST_TOKEN_P50_MAX_MS} ms)"
        ));
    }
    Ok(())
}

/// The content chunks of an OpenAI-style event stream, in order: the oracle that both paths did the same work.
pub fn content_deltas(stream: &str) -> Vec<String> {
    stream
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .filter_map(|event| {
            event["choices"][0]["delta"]["content"]
                .as_str()
                .map(str::to_owned)
        })
        .collect()
}

#[cfg(test)]
#[path = "pairs_tests.rs"]
mod tests;
