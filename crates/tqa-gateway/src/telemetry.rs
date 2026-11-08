use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use metrics::{Key, Label, Metadata, Recorder};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusRecorder};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tqa_types::{Event, ServerFrame};

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;

const METADATA: Metadata<'static> =
    Metadata::new("tqa_gateway", metrics::Level::INFO, Some("tqa_gateway"));

/// How often the histograms' samples are folded into their buckets: the exporter's own default upkeep period.
pub(crate) const UPKEEP_EVERY: Duration = Duration::from_secs(5);

pub(crate) struct Telemetry {
    recorder: PrometheusRecorder,
    /// Upkeeps run so far (GW-U-177).
    upkeeps: AtomicU64,
}

impl Telemetry {
    pub fn new() -> Result<Self, crate::GatewayError> {
        let builder = PrometheusBuilder::new()
            .set_buckets(&[0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0])
            .map_err(|_| crate::GatewayError::Configuration("invalid metric buckets"))?;
        Ok(Self {
            recorder: builder.build_recorder(),
            upkeeps: AtomicU64::new(0),
        })
    }

    /// Folds every histogram's recorded samples into its buckets, which frees them. A recorder made with
    /// `build_recorder` runs no upkeep of its own and keeps each sample until a scrape renders it, so with metrics off
    /// or never scraped every request's sample stayed for the gateway's life (the c15 soak's heap growth, 9 Oct 2026;
    /// GW-U-177).
    pub fn upkeep(&self) {
        self.recorder.handle().run_upkeep();
        self.upkeeps.fetch_add(1, Ordering::Relaxed);
    }

    /// Upkeeps run so far.
    #[cfg(test)]
    pub fn upkeeps(&self) -> u64 {
        self.upkeeps.load(Ordering::Relaxed)
    }

    pub fn request(&self, method: &'static str, route: &'static str, status: u16, seconds: f64) {
        let labels = vec![
            Label::new("method", method),
            Label::new("route", route),
            Label::new("status", status.to_string()),
        ];
        self.recorder
            .register_counter(
                &Key::from_parts("tqa_http_requests_total", labels.clone()),
                &METADATA,
            )
            .increment(1);
        self.recorder
            .register_histogram(
                &Key::from_parts("tqa_http_request_duration_seconds", labels),
                &METADATA,
            )
            .record(seconds);
    }

    /// Seconds a relayed request waited for its model route's limiter (LLM-U-012); route names are admin-defined.
    pub fn relay_queue(&self, route: &str, seconds: f64) {
        let labels = vec![Label::new("route", route.to_owned())];
        self.recorder
            .register_histogram(
                &Key::from_parts("tqa_relay_queue_seconds", labels),
                &METADATA,
            )
            .record(seconds);
    }

    /// One relayed call recorded with no run (the run-id contract): `missing`, the call named none (an older runtime),
    /// or `foreign`, it named one that isn't a run of the caller's task. Labels stay bounded: a route or engine name.
    pub fn relay_unattributed(&self, route: &str, reason: &'static str) {
        let labels = vec![
            Label::new("route", route.to_owned()),
            Label::new("reason", reason),
        ];
        self.recorder
            .register_counter(
                &Key::from_parts("tqa_relay_unattributed_total", labels),
                &METADATA,
            )
            .increment(1);
    }

    /// One relay throttle (ADR-0025); labels stay bounded: an admin-defined route name and `user` or `route`.
    pub fn relay_throttled(&self, route: &str, scope: &'static str) {
        let labels = vec![
            Label::new("route", route.to_owned()),
            Label::new("scope", scope),
        ];
        self.recorder
            .register_counter(
                &Key::from_parts("tqa_relay_throttled_total", labels),
                &METADATA,
            )
            .increment(1);
    }

    /// One sandbox retired by a reaper rule; the reason is a fixed word (`lifetime`), never an identifier.
    pub fn sandbox_retired(&self, reason: &'static str) {
        self.recorder
            .register_counter(
                &Key::from_parts(
                    "tqa_sandbox_retired_total",
                    vec![Label::new("reason", reason)],
                ),
                &METADATA,
            )
            .increment(1);
    }

    /// Subnets left in the per-sandbox network pool (ADR-0021 amendment).
    pub fn network_pool_free(&self, free: u32) {
        self.recorder
            .register_gauge(&Key::from_name("tqa_sandbox_network_pool_free"), &METADATA)
            .set(f64::from(free));
    }

    /// A keyed call the proxy saw (ADR-0037), by its outcome: `ok` for a swap, else the refusal's reason. The label
    /// is one of a fixed few, so the series stay bounded.
    pub fn host_key_use(&self, outcome: &'static str) {
        self.recorder
            .register_counter(
                &Key::from_parts(
                    "tqa_host_key_uses_total",
                    vec![Label::new("outcome", outcome)],
                ),
                &METADATA,
            )
            .increment(1);
    }

    /// A secret read whose audit row could not be written in time; a key read then is not used (GW-I-147).
    pub fn secret_read_unrecorded(&self) {
        self.recorder
            .register_counter(
                &Key::from_name("tqa_secret_reads_unrecorded_total"),
                &METADATA,
            )
            .increment(1);
    }

    /// How many secrets are due for rotation (vault slice 1).
    pub fn secrets_rotation_due(&self, count: u64) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a count of secrets, bounded far below 2^52"
        )]
        let count = count as f64;
        self.recorder
            .register_gauge(&Key::from_name("tqa_secrets_rotation_due"), &METADATA)
            .set(count);
    }

    /// Purge targets handled (delete means gone): `kind` is `task` or `workspace`, `outcome` is `done`, `deferred`
    /// (not ready: a live sandbox or link, or a workspace's tasks still there) or `failed` (a step failed; retried next
    /// cycle). Fixed words only, never an identifier.
    pub fn purge_targets(&self, kind: &'static str, outcome: &'static str, count: u64) {
        self.recorder
            .register_counter(
                &Key::from_parts(
                    "tqa_purge_targets_total",
                    vec![Label::new("kind", kind), Label::new("outcome", outcome)],
                ),
                &METADATA,
            )
            .increment(count);
    }

    /// The age in seconds of the oldest deleted task or workspace not yet purged; 0 when none is waiting. "Within
    /// minutes" is checked and alerted on with it.
    pub fn purge_oldest(&self, seconds: f64) {
        self.recorder
            .register_gauge(&Key::from_name("tqa_purge_oldest_seconds"), &METADATA)
            .set(seconds);
    }

    /// A create found the per-sandbox network pool exhausted.
    pub fn network_pool_exhausted(&self) {
        self.recorder
            .register_counter(
                &Key::from_name("tqa_sandbox_network_pool_exhausted_total"),
                &METADATA,
            )
            .increment(1);
    }

    /// One MCP proxy request: the connector (at most 64), the method label (fixed) and the outcome (a fixed code).
    pub fn mcp_proxy_request(&self, connector: &str, method: &'static str, outcome: &'static str) {
        self.recorder
            .register_counter(
                &Key::from_parts(
                    "tqa_mcp_proxy_requests_total",
                    vec![
                        Label::new("connector", connector.to_owned()),
                        Label::new("method", method),
                        Label::new("outcome", outcome),
                    ],
                ),
                &METADATA,
            )
            .increment(1);
    }

    pub fn event(&self, frame: &ServerFrame) {
        let ServerFrame::Event { event, ts, .. } = frame else {
            return;
        };
        let result = match event {
            Event::RunCompleted { .. } => Some("completed"),
            Event::RunCancelled { .. } => Some("cancelled"),
            Event::RunFailed { .. } => Some("failed"),
            _ => None,
        };
        if let Some(result) = result {
            self.recorder
                .register_counter(
                    &Key::from_parts("tqa_runs_total", vec![Label::new("result", result)]),
                    &METADATA,
                )
                .increment(1);
        }
        if let Ok(timestamp) =
            time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339)
        {
            let seconds = (time::OffsetDateTime::now_utc() - timestamp)
                .as_seconds_f64()
                .max(0.0);
            self.recorder
                .register_histogram(&Key::from_name("tqa_event_lag_seconds"), &METADATA)
                .record(seconds);
        }
    }

    /// How many frames one journal transaction stored (1 for a single append), so batching shows in the metrics.
    pub fn batch(&self, frames: usize) {
        #[allow(
            clippy::cast_precision_loss,
            reason = "a frame count, at most a batch's 256"
        )]
        self.recorder
            .register_histogram(&Key::from_name("tqa_event_batch_frames"), &METADATA)
            .record(frames as f64);
    }

    pub fn render(&self) -> String {
        self.recorder.handle().render()
    }
}

/// Runs [`Telemetry::upkeep`] every [`UPKEEP_EVERY`] until the gateway stops (GW-U-177).
pub(crate) fn start_upkeep(
    telemetry: Arc<Telemetry>,
    cancel: CancellationToken,
    tasks: &TaskTracker,
) {
    tasks.spawn(async move {
        let mut every = tokio::time::interval(UPKEEP_EVERY);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                _ = every.tick() => telemetry.upkeep(),
            }
        }
    });
}
