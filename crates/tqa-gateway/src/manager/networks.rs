//! Docker sandboxes' own networks (ADR-0021 amendment): reconciled at start (the gateway rejoins live sandboxes'
//! networks, orphans are removed, sandboxes left on the former shared network are retired and resume in isolated
//! ones), swept periodically and at once when the subnet pool runs out.
use super::{SandboxManager, failure};
use crate::GatewayError;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;

/// How often orphaned sandbox networks and stray sandbox containers (GW-I-191) are looked for, besides at start and,
/// for networks, on exhaustion.
const NETWORK_SWEEP: Duration = Duration::from_mins(5);

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
            let mut timer = tokio::time::interval(NETWORK_SWEEP);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            timer.tick().await; // the start-up reconciliation has just run
            loop {
                tokio::select! {biased;
                    () = manager.services.cancel.cancelled() => return,
                    () = manager.networks.wake.notified() => {}
                    _ = timer.tick() => {}
                }
                manager.sweep_strays().await;
                manager.sweep_networks().await;
            }
        });
    }

    async fn sweep_strays(&self) {
        match self.sweep_containers(std::time::SystemTime::now()).await {
            Ok(sweep) if sweep.removed > 0 || sweep.failed > 0 => tracing::warn!(
                removed = sweep.removed,
                failed = sweep.failed,
                "stray sandbox containers swept"
            ),
            Ok(_) => {}
            Err(error) => tracing::warn!(code = error.code(), "sandbox container sweep failed"),
        }
    }

    async fn sweep_networks(&self) {
        let Some(docker) = &self.backend.docker else {
            return;
        };
        match docker.reconcile_networks(false).await {
            Ok(sweep) if sweep.removed > 0 || sweep.failed > 0 => tracing::warn!(
                removed = sweep.removed,
                failed = sweep.failed,
                "orphaned sandbox networks swept"
            ),
            Ok(_) => {}
            Err(error) => tracing::warn!(code = error.code(), "sandbox network sweep failed"),
        }
        self.report_pool();
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
