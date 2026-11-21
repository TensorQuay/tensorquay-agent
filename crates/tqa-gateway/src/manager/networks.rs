//! Docker sandboxes' own networks (ADR-0021 amendment): reconciled at start (the gateway rejoins live sandboxes'
//! networks, orphans are removed, sandboxes left on the former shared network are retired and resume in isolated
//! ones), swept periodically and at once when the subnet pool runs out.
use super::{SandboxManager, failure, verdict};
use crate::GatewayError;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

#[derive(Default)]
pub(super) struct Sweeper {
    wake: Notify,
    /// Set while the pool is exhausted, so the ERROR is logged once per episode.
    exhausted: AtomicBool,
}

impl SandboxManager {
    /// At start, before records are reconciled: rejoin, remove orphans, and retire sandboxes on the former shared
    /// network (their records are then reset and their runs resume in new, isolated sandboxes).
    ///
    /// # Errors
    /// Fails closed when the engine cannot be listed or a legacy sandbox cannot be retired.
    pub(super) async fn reconcile_networks(&self) -> Result<(), GatewayError> {
        let Some(docker) = &self.backend.docker else {
            return Ok(());
        };
        let sweep = docker
            .reconcile_networks(true)
            .await
            .map_err(failure("inventory"))?;
        for handle in &sweep.legacy {
            tracing::info!(
                conversation_id = %handle.conversation_id,
                "retiring a sandbox on the former shared network; its run resumes in an isolated one"
            );
            self.backend
                .engine
                .destroy(handle)
                .await
                .map_err(failure("destroy"))?;
        }
        tracing::info!(
            reattached = sweep.reattached,
            removed = sweep.removed,
            failed = sweep.failed,
            retired = sweep.legacy.len(),
            "sandbox networks reconciled"
        );
        self.report_pool();
        Ok(())
    }

    /// Renews what internal egress rules may not reach, at once and every [`REFRESH`]: the engine's networks and the
    /// gateway's own addresses. A failed renewal keeps the last one until it goes stale, and internal rules then refuse.
    ///
    /// [`REFRESH`]: crate::egress::internal::REFRESH
    pub(crate) fn start_internal_inventory(self: &Arc<Self>) {
        let manager = self.clone();
        self.services.tasks.spawn(async move {
            let mut timer = tokio::time::interval(crate::egress::internal::REFRESH);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {biased;
                    () = manager.services.cancel.cancelled() => return,
                    _ = timer.tick() => {}
                }
                manager.renew_internal_inventory().await;
            }
        });
    }

    pub(crate) async fn renew_internal_inventory(&self) {
        let guard = &self.services.internal;
        let networks = async {
            self.backend
                .engine
                .engine_networks()
                .await
                .map_err(|_| crate::GatewayError::EngineUnavailable)
        };
        match self.bounded(networks).await {
            Ok(engine) => {
                let own = guard.own();
                let inventory = crate::egress::internal::Inventory::new(
                    &engine,
                    &own,
                    &guard.fixed(),
                    std::time::Instant::now(),
                );
                guard.update(inventory);
            }
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "the engine's networks could not be read; internal egress rules refuse until they can"
                );
            }
        }
    }

    /// Starts the owned periodic sweep; it stops with the gateway. Stray containers are swept on every backend; only
    /// Docker sandboxes have networks of their own.
    pub(crate) fn start_network_sweep(self: &Arc<Self>) {
        let manager = self.clone();
        self.services.tasks.spawn(async move {
            // Orphaned sandbox networks and stray sandbox containers (GW-I-191) are looked for every `[sandbox]
            // sweep_seconds` (GW-U-233; 5 minutes by default), besides at start and, for networks, on exhaustion.
            let mut timer = tokio::time::interval(manager.config.sandbox.sweep_period());
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            timer.tick().await; // the start-up reconciliation has just run
            loop {
                tokio::select! {biased;
                    () = manager.services.cancel.cancelled() => return,
                    () = manager.networks.wake.notified() => {}
                    _ = timer.tick() => {}
                }
                manager.sweep_pass_now().await;
                // The next pass is a full period after this one ends. A pass slower than the period (a stray's stop
                // takes about 10 s) otherwise met its missed tick at once, and a pass that found nothing left replaced
                // the slow pass's verdict within a millisecond (GW-I-373).
                timer.reset();
            }
        });
    }

    /// Runs one sweep pass now (stray containers, then orphaned networks) and records it for the leak verdict. The
    /// owned sweep runs the same pass every `sweep_seconds`; this entry point exists so a caller can wait for one
    /// deterministically.
    pub async fn sweep_pass_now(&self) {
        let mut findings = self.sweep_strays().await;
        findings.extend(self.sweep_networks().await);
        self.record_sweep(findings).await;
    }

    /// Keeps the pass for the leak verdict (the Sandboxes page design note, §3.6), with what runs now.
    async fn record_sweep(&self, findings: Vec<verdict::Finding>) {
        let (running, oldest_seconds) = self.running_count().await.unwrap_or((0, 0));
        if let Ok(mut sweeps) = self.sweeps.lock() {
            sweeps.record(verdict::Pass {
                at: time::OffsetDateTime::now_utc(),
                running,
                oldest_seconds,
                // No volume is created or labelled by the gateway today, so none is looked at (see the note's §3.6).
                volumes: false,
                findings,
            });
        }
    }

    /// How many sandboxes have a record, and the oldest one's age in seconds.
    pub(crate) async fn running_count(&self) -> Result<(u64, u64), GatewayError> {
        let row = self
            .bounded(async {
                Ok(sqlx::query!(
                    "SELECT count(*) AS \"running!\", COALESCE(EXTRACT(EPOCH FROM (CURRENT_TIMESTAMP - MIN(created_at))), 0)::bigint AS \"oldest!\" FROM sandbox_instances"
                )
                .fetch_one(&self.services.pool)
                .await?)
            })
            .await?;
        Ok((
            u64::try_from(row.running).unwrap_or(0),
            u64::try_from(row.oldest).unwrap_or(0),
        ))
    }

    /// The leak verdict as of the last sweep.
    pub(crate) fn leak_verdict(&self) -> verdict::Verdict {
        self.sweeps
            .lock()
            .map_or_else(|_| verdict::Verdict::default(), |sweeps| sweeps.verdict())
    }

    /// The readiness detail's `sandboxes` object, with the live running count, and the gauges set from it.
    pub(crate) async fn sandboxes_readiness(&self) -> serde_json::Value {
        let (running, oldest) = self.running_count().await.unwrap_or((0, 0));
        let verdict = self.leak_verdict().with_running(running, oldest);
        let states = self.state_counts().await.unwrap_or_default();
        self.services
            .telemetry
            .sandboxes(self.config.backend_name(), &states, &verdict);
        verdict.readiness()
    }

    /// How many sandboxes are starting, working, idle and stopping, for the `tqa_sandboxes` gauge.
    async fn state_counts(&self) -> Result<Vec<(&'static str, u64)>, GatewayError> {
        let rows = self
            .bounded(async {
                Ok(sqlx::query!(
                    "SELECT s.phase, COALESCE((SELECT e.lifecycle FROM events e WHERE e.conversation_id=s.conversation_id AND e.lifecycle IS NOT NULL ORDER BY e.seq DESC LIMIT 1) IN ('run.started','run.resumed'), false) AS \"working!\" FROM sandbox_instances s"
                )
                .fetch_all(&self.services.pool)
                .await?)
            })
            .await?;
        let mut counts = [
            ("starting", 0u64),
            ("working", 0),
            ("idle", 0),
            ("stopping", 0),
        ];
        for row in rows {
            let index = match row.phase.as_str() {
                "starting" => 0,
                "destroying" => 3,
                _ if row.working => 1,
                _ => 2,
            };
            counts[index].1 += 1;
        }
        Ok(counts.to_vec())
    }

    async fn sweep_strays(&self) -> Vec<verdict::Finding> {
        match self.sweep_containers(std::time::SystemTime::now()).await {
            Ok(sweep) => {
                if sweep.removed > 0 || sweep.failed > 0 {
                    tracing::warn!(
                        removed = sweep.removed,
                        failed = sweep.failed,
                        "stray sandbox containers swept"
                    );
                }
                sweep.findings
            }
            Err(error) => {
                tracing::warn!(code = error.code(), "sandbox container sweep failed");
                Vec::new()
            }
        }
    }

    async fn sweep_networks(&self) -> Vec<verdict::Finding> {
        let Some(docker) = &self.backend.docker else {
            return Vec::new();
        };
        let findings =
            match docker.reconcile_networks(false).await {
                Ok(sweep) => {
                    if sweep.removed > 0 || sweep.failed > 0 {
                        tracing::warn!(
                            removed = sweep.removed,
                            failed = sweep.failed,
                            "orphaned sandbox networks swept"
                        );
                    }
                    sweep
                        .removed_names
                        .into_iter()
                        .map(|name| verdict::Finding {
                            kind: verdict::Kind::Network,
                            name,
                            age_seconds: 0,
                            outcome: Ok(()),
                        })
                        .chain(sweep.failed_names.into_iter().map(|(name, reason)| {
                            verdict::Finding {
                                kind: verdict::Kind::Network,
                                name,
                                age_seconds: 0,
                                outcome: Err(reason),
                            }
                        }))
                        .collect()
                }
                Err(error) => {
                    tracing::warn!(code = error.code(), "sandbox network sweep failed");
                    Vec::new()
                }
            };
        self.report_pool();
        findings
    }

    /// A create found no free subnet: the work stays queued, the episode is logged once, and a sweep runs now.
    pub(super) fn pool_exhausted(&self) {
        if !self.networks.exhausted.swap(true, Ordering::AcqRel) {
            tracing::error!(
                "the sandbox network pool is exhausted; new sandboxes wait while leaked networks are swept"
            );
        }
        self.services.telemetry.network_pool_exhausted();
        self.networks.wake.notify_one();
    }

    /// Publishes the free subnets; a pool with room again ends an exhaustion episode.
    pub(super) fn report_pool(&self) {
        let Some(docker) = &self.backend.docker else {
            return;
        };
        if let Ok(free) = docker.pool_free() {
            self.services.telemetry.network_pool_free(free);
            if free > 0 {
                self.networks.exhausted.store(false, Ordering::Release);
            }
        }
    }
}
