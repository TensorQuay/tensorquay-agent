//! Runtime-mutable settings (ADR-0028): one immutable snapshot, swapped atomically and never backwards.
//!
//! A reader clones the current `Arc<Snapshot>` and releases the lock at once, so no guard is held across an `.await`, and
//! uses that one snapshot for its whole operation: no request sees half a change. Writers rebuild a snapshot from the
//! database and install it under one async mutex; a snapshot only replaces an older generation, so two rebuilds that
//! finish out of order can never leave the older one live. The snapshot is coherent for one gateway process.
mod audit;
pub(crate) mod builtins;
pub(crate) mod connectors;
pub(crate) mod decision_engines;
mod digest;
pub(crate) mod egress;
pub(crate) mod egress_presets;
pub(crate) mod limit_rows;
pub(crate) mod limits;
mod route_reasons;
pub(crate) mod routes;
mod template_results;
pub(crate) mod templates;
pub(crate) mod tombstones;

pub(crate) use audit::{Change, audit, audit_builtin, audit_system, audit_user};
pub(crate) use digest::KeyedDigest;

use std::sync::{Arc, PoisonError, RwLock};

/// The settings in force for one operation: routes and the egress allow-list; limit overrides join them later
/// (ADR-0028 slice 5).
#[derive(Debug)]
pub(crate) struct Snapshot {
    /// Increases with every committed settings change; the files alone are generation 0.
    pub(crate) generation: i64,
    /// The model routes.
    pub(crate) registry: tqa_llm::Registry,
    /// Route labels for the pickers, by route name; a route without one shows its name.
    pub(crate) display_names: std::collections::BTreeMap<String, String>,
    /// Route descriptions by name, for the pickers (GW-I-132).
    pub(crate) descriptions: std::collections::BTreeMap<String, String>,
    /// The policy proxy's allow-list; empty denies everything.
    pub(crate) egress: crate::egress::AllowList,
    /// The effective overridable limits (ADR-0028 slice 5): read once per admission pass or sandbox creation.
    pub(crate) limits: limits::Limits,
    /// The connectors (ADR-0031), by name.
    pub(crate) connectors: Vec<crate::connectors::ConnectorDocument>,
    /// The decision engines (ADR-0013 point 6), by name.
    pub(crate) decision_engines: Vec<crate::decision_engines::EngineDocument>,
    /// The environment templates (ADR-0032), by name.
    pub(crate) templates: Vec<templates::StoredTemplate>,
    /// Which routes and connectors may use each shared secret.
    pub(crate) bindings: crate::secrets::bindings::Bindings,
}

/// The shared, swappable settings.
pub(crate) struct Settings {
    current: RwLock<Arc<Snapshot>>,
    swap: tokio::sync::Mutex<()>,
    /// Woken after a newer snapshot is installed (the template checker waits on it).
    changed: tokio::sync::Notify,
}

impl Settings {
    /// Applies the seeds (the models file's routes and `[[egress.allow]]`) and builds the first snapshot from the
    /// database, in one settings transaction.
    ///
    /// # Errors
    /// Storage failure, deadlines, an unusable install key, or invalid stored settings.
    pub(crate) async fn load(
        config: &crate::GatewayConfig,
        secrets: &crate::GatewaySecrets,
        pool: &sqlx::PgPool,
        cancel: &tokio_util::sync::CancellationToken,
        seed: tqa_llm::Registry,
    ) -> Result<Arc<Self>, crate::GatewayError> {
        let digest = crate::secrets::seed_digest(config, secrets, cancel).await?;
        let snapshot = crate::io::bounded(config.io_timeout(), cancel, async {
            let mut transaction = pool.begin().await?;
            lock(&mut transaction).await?;
            // Pasted keys leave the database before anything else reads the routes (GW-I-097).
            routes::scrub(&mut transaction).await?;
            let routes = routes::seed(&mut transaction, &seed, &digest).await?;
            let rules = egress::seed(&mut transaction, &config.egress.seed, &digest).await?;
            let limits = limit_rows::seed(
                &mut transaction,
                &file_limits(config),
                config.sandbox.max_live,
                &digest,
            )
            .await?;
            let connectors =
                connectors::seed(&mut transaction, &config.connectors, &digest).await?;
            let engines =
                decision_engines::seed(&mut transaction, &config.decision_engines, &digest).await?;
            let templates = templates::seed(
                &mut transaction,
                (config.templates(), &config.sandbox.ceiling),
                &digest,
            )
            .await?;
            if routes || rules || limits || connectors || engines || templates {
                bump(&mut transaction).await?;
            }
            let snapshot = read(&mut transaction).await?;
            // Shared profiles are not in the snapshot; their names are checked against it (ADR-0012).
            let servers = config.mcp.servers();
            crate::profiles::seed(
                &mut transaction,
                config.profiles(),
                &digest,
                (&snapshot, servers),
            )
            .await?;
            transaction.commit().await?;
            Ok(snapshot)
        })
        .await?;
        tracing::info!(generation = snapshot.generation, "settings loaded");
        Ok(Arc::new(Self::new(snapshot)))
    }

    /// Rebuilds the snapshot from the database and installs it unless a newer one is already live. Call it after
    /// committing a settings write (and after replaying one), so the change applies from the next operation.
    ///
    /// # Errors
    /// Storage failure, deadlines, or invalid stored settings; the current snapshot then stays live.
    pub(crate) async fn refresh(
        &self,
        pool: &sqlx::PgPool,
        timeout: std::time::Duration,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<(), crate::GatewayError> {
        let _rebuilding = self.rebuilding().await;
        let snapshot = crate::io::bounded(timeout, cancel, async {
            let mut transaction = pool.begin().await?;
            sqlx::query!("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
                .execute(&mut *transaction)
                .await?;
            let snapshot = read(&mut transaction).await?;
            transaction.commit().await?;
            Ok(snapshot)
        })
        .await?;
        let generation = snapshot.generation;
        if self.install(snapshot) {
            tracing::info!(generation, "settings snapshot installed");
        }
        Ok(())
    }

    pub(crate) fn new(snapshot: Snapshot) -> Self {
        Self {
            current: RwLock::new(Arc::new(snapshot)),
            swap: tokio::sync::Mutex::new(()),
            changed: tokio::sync::Notify::new(),
        }
    }

    /// The snapshot to use for one whole operation. The lock is released before this returns.
    pub(crate) fn current(&self) -> Arc<Snapshot> {
        // A poisoned lock still holds a complete snapshot: a panic cannot interrupt the single pointer store below.
        self.current.read().map_or_else(
            |poisoned| Arc::clone(&poisoned.into_inner()),
            |guard| Arc::clone(&guard),
        )
    }

    /// Serialises rebuild-and-install; hold it from reading the database to installing the result.
    pub(crate) async fn rebuilding(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.swap.lock().await
    }

    /// Installs `next` only if it is newer than the current snapshot; returns whether it did.
    pub(crate) fn install(&self, next: Snapshot) -> bool {
        let mut current = self.current.write().unwrap_or_else(PoisonError::into_inner);
        if next.generation <= current.generation {
            return false;
        }
        *current = Arc::new(next);
        drop(current);
        self.changed.notify_waiters();
        true
    }

    /// Resolves after the next snapshot install; enable it before the work it must not miss.
    pub(crate) fn changed(&self) -> tokio::sync::futures::Notified<'_> {
        self.changed.notified()
    }
}

/// Serialises every settings write and rebuild across the settings areas and gateway processes.
///
/// # Errors
/// Storage failure.
pub(crate) async fn lock(connection: &mut sqlx::PgConnection) -> Result<(), crate::GatewayError> {
    crate::db::queue(connection, crate::db::Queue::Named("tqa.settings")).await?;
    Ok(())
}

/// Bumps and returns the settings generation in the caller's transaction; every settings write does this once.
///
/// # Errors
/// Storage failure.
pub(crate) async fn bump(connection: &mut sqlx::PgConnection) -> Result<i64, crate::GatewayError> {
    Ok(sqlx::query_scalar!(
        "UPDATE settings_generation SET generation = generation + 1 WHERE id = 1 RETURNING generation"
    )
    .fetch_one(connection)
    .await?)
}

/// The committed settings generation.
///
/// # Errors
/// Storage failure.
pub(crate) async fn generation(
    connection: &mut sqlx::PgConnection,
) -> Result<i64, crate::GatewayError> {
    Ok(
        sqlx::query_scalar!("SELECT generation FROM settings_generation WHERE id = 1")
            .fetch_one(connection)
            .await?,
    )
}

/// Every settings area at one generation, from one transaction.
async fn read(connection: &mut sqlx::PgConnection) -> Result<Snapshot, crate::GatewayError> {
    let generation = generation(&mut *connection).await?;
    Ok(Snapshot {
        generation,
        registry: routes::registry(&mut *connection).await?,
        display_names: routes::display_names(&mut *connection).await?,
        descriptions: routes::descriptions(&mut *connection).await?,
        egress: egress::allow_list(&mut *connection).await?,
        limits: limit_rows::effective(&mut *connection).await?,
        connectors: connectors::documents(&mut *connection).await?,
        decision_engines: decision_engines::documents(&mut *connection).await?,
        templates: templates::read(&mut *connection).await?,
        bindings: crate::secrets::bindings::read(connection).await?,
    })
}

/// The limits the configuration file sets (with its defaults applied): the seed of the limit overrides.
pub(crate) const fn file_limits(config: &crate::GatewayConfig) -> limits::Limits {
    limits::Limits {
        slots: config.admission.slots,
        per_user: config.admission.per_user,
        idle_seconds: config.sandbox.idle_seconds,
        max_lifetime_seconds: config.sandbox.max_lifetime_seconds,
        mcp_calls_per_minute: config.mcp.calls_per_minute(),
        team_max_members: config.agent_teams.max_members,
        team_token_budget: config.agent_teams.token_budget,
        team_deadline_minutes: config.agent_teams.deadline_minutes,
        member_idle_minutes: config.agent_teams.member_idle_minutes,
        member_stuck_minutes: config.agent_teams.member_stuck_minutes,
        team_idle_stop_minutes: config.agent_teams.idle_stop_minutes,
        desktop_max_sessions: config.desktop.max_sessions,
        desktop_screenshot_retention_days: config.desktop.screenshot_retention_days,
        computer_max_screenshots_per_run: config.computer.max_screenshots_per_run,
        computer_max_actions_per_run: config.computer.max_actions_per_run,
    }
}

#[cfg(test)]
#[path = "settings/tests.rs"]
mod tests;
