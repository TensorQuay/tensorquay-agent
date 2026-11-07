//! Forward-only schema and the bounded `PostgreSQL` connection boundary.

use crate::{GatewayConfig, GatewayError, GatewaySecrets};
use secrecy::ExposeSecret;
use sqlx::{
    Connection, PgConnection, PgPool,
    postgres::{PgConnectOptions, PgDatabaseError, PgPoolOptions},
};
use std::{
    str::FromStr,
    sync::{Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

/// The checked-in forward-only migrations applied before serving requests.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// How long a pooled connection stays idle, and lives at most, before the pool replaces it: sqlx's own defaults,
/// written here so that a library update can't move them.
pub(crate) const POOL_IDLE: Duration = Duration::from_mins(10);
pub(crate) const POOL_LIFETIME: Duration = Duration::from_mins(30);

/// SQLSTATE `lock_not_available`: a statement waited for a lock longer than `lock_timeout`.
const LOCK_NOT_AVAILABLE: &str = "55P03";

/// The pool that names a lock timeout's holder, set once by [`connect`] (one gateway per process).
static HOLDERS: OnceLock<PgPool> = OnceLock::new();

/// The `application_name` of every connection the gateway opens. A holder's statement is shown only for sessions
/// with this name: another client's statement (an operator's psql under the same role) may hold literal values.
pub(crate) const APPLICATION: &str = "tqa-gateway";

/// At most one holder read at a time, and one per this interval: a stuck row with many waiters gives many lock
/// timeouts, and each read takes a pool connection that requests need in the middle of the incident.
pub(crate) const HOLDER_READ_INTERVAL: Duration = Duration::from_secs(5);
static HOLDER_READS: HolderReads = HolderReads::new();

/// A pooled statement waits for a lock half the I/O bound (5 s at the default 10 s), so a held row ends the wait
/// with SQLSTATE 55P03, which names the row and lets its holder be read, before the statement bound would end it
/// as a bare timeout.
pub(crate) fn lock_timeout(io: Duration) -> Duration {
    (io / 2).max(Duration::from_millis(500))
}

/// The server ends a session idle inside a transaction for three I/O bounds. Every bounded transaction is dropped
/// at one bound and rolled back when its connection returns to the pool, so only one that escaped its bound waits
/// that long, holding its rows.
pub(crate) fn idle_in_transaction_timeout(io: Duration) -> Duration {
    io.saturating_mul(3)
}

/// The settings every connection gets: the statement bound, UTC and the gateway's application name.
fn base_options(
    config: &GatewayConfig,
    secrets: &GatewaySecrets,
) -> Result<PgConnectOptions, GatewayError> {
    let timeout_ms = config.io_timeout().as_millis().to_string();
    Ok(
        PgConnectOptions::from_str(secrets.database_url.expose_secret())
            .map_err(|_| GatewayError::Configuration("database connection options are invalid"))?
            .application_name(APPLICATION)
            .options([
                ("statement_timeout", timeout_ms.as_str()),
                ("timezone", "UTC"),
            ]),
    )
}

/// The pool's connections add the lock and idle-transaction bounds. The migration connection doesn't: a migration
/// may wait for a lock on purpose (another gateway migrating holds sqlx's advisory lock).
pub(crate) fn pooled_options(base: PgConnectOptions, io: Duration) -> PgConnectOptions {
    let lock = lock_timeout(io).as_millis().to_string();
    let idle = idle_in_transaction_timeout(io).as_millis().to_string();
    base.options([
        ("lock_timeout", lock.as_str()),
        ("idle_in_transaction_session_timeout", idle.as_str()),
    ])
}

pub(crate) fn pool_options(config: &GatewayConfig) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(config.limits.database_connections)
        .acquire_timeout(config.io_timeout())
        .idle_timeout(POOL_IDLE)
        .max_lifetime(POOL_LIFETIME)
}

pub(crate) async fn connect(
    config: &GatewayConfig,
    secrets: &GatewaySecrets,
    cancel: &CancellationToken,
) -> Result<PgPool, GatewayError> {
    let base = base_options(config, secrets)?;
    let pooled = pooled_options(base.clone(), config.io_timeout());
    let pool = named(
        "database connection",
        Box::pin(crate::io::bounded(config.io_timeout(), cancel, async {
            pool_options(config)
                .connect_with(pooled)
                .await
                .map_err(GatewayError::from)
        })),
    )
    .await?;
    named("database migrations", migrate_alone(config, &base, cancel)).await?;
    // A second connect in the same process (tests) keeps the first pool here; the report then says it couldn't read.
    let _ = HOLDERS.set(pool.clone());
    Ok(pool)
}

/// Names a start-up step's failure, as `app::step` does for the steps after the database (GW-U-060, GW-U-071).
async fn named<T>(
    step: &'static str,
    future: impl std::future::Future<Output = Result<T, GatewayError>>,
) -> Result<T, GatewayError> {
    future
        .await
        .map_err(|error| crate::startup::failed(step, error))
}

pub(crate) async fn migrate(
    config: &GatewayConfig,
    pool: &PgPool,
    cancel: &CancellationToken,
) -> Result<(), GatewayError> {
    crate::io::bounded(config.io_timeout(), cancel, async {
        MIGRATOR
            .run(pool)
            .await
            .map_err(|_| GatewayError::StoreUnavailable)
    })
    .await
}

/// The migrations on a connection of their own, without the pool's lock bound.
async fn migrate_alone(
    config: &GatewayConfig,
    options: &PgConnectOptions,
    cancel: &CancellationToken,
) -> Result<(), GatewayError> {
    crate::io::bounded(config.io_timeout(), cancel, async {
        let mut connection = PgConnection::connect_with(options).await?;
        MIGRATOR
            .run(&mut connection)
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        connection.close().await.map_err(GatewayError::from)
    })
    .await
}

/// A statement that waited past `lock_timeout`, with the row it waited for when the server named one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LockTimeout {
    /// The server's context line, for example `while updating tuple (0,5) in relation "workspaces"`.
    pub(crate) context: Option<String>,
    /// The relation named in that line.
    pub(crate) relation: Option<String>,
}

impl LockTimeout {
    /// The lock timeout inside `error`, if that is what it is.
    pub(crate) fn of(error: &sqlx::Error) -> Option<Self> {
        let database = error
            .as_database_error()?
            .try_downcast_ref::<PgDatabaseError>()?;
        (database.code() == LOCK_NOT_AVAILABLE).then(|| {
            let context = database.r#where().map(str::to_owned);
            let relation = context.as_deref().and_then(relation_of);
            Self { context, relation }
        })
    }
}

/// The relation in a lock wait's context line: `... in relation "name"`.
fn relation_of(context: &str) -> Option<String> {
    let (_, rest) = context.split_once(" in relation \"")?;
    let (name, _) = rest.split_once('"')?;
    Some(name.to_owned())
}

/// A session that held what a timed-out statement waited for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Holder {
    pub(crate) pid: i32,
    /// It blocks a session still waiting for a lock (`pg_blocking_pids`). Otherwise it only holds a lock on the named
    /// table, which may or may not be the row the statement waited for: by the time it's read, that wait is over.
    pub(crate) blocking: bool,
    /// `pg_stat_activity.state`, for example `idle in transaction`.
    pub(crate) state: Option<String>,
    /// Seconds since its transaction began.
    pub(crate) transaction_seconds: Option<f64>,
    /// Seconds since its state last changed (for an idle transaction: how long it has sent nothing).
    pub(crate) state_seconds: Option<f64>,
    pub(crate) wait_event: Option<String>,
    /// Its last statement's first 160 characters, for the gateway's own sessions only (their values are bound
    /// parameters, so the text holds none); another client's reads "another client (statement not shown)".
    pub(crate) query: Option<String>,
}

/// The sessions holding what a timed-out statement waited for, read on another pool connection: whoever blocks a
/// session still waiting for a lock now (`pg_blocking_pids`), and whoever holds a lock on the named relation, each
/// marked as which. Real blockers first, then the oldest transaction, at most three.
pub(crate) async fn held_by(
    pool: &PgPool,
    relation: Option<&str>,
) -> Result<Vec<Holder>, sqlx::Error> {
    sqlx::query_as!(
        Holder,
        r#"WITH found AS (
               SELECT unnest(pg_blocking_pids(pid)) AS pid, true AS blocking FROM pg_stat_activity
               WHERE datname = current_database() AND wait_event_type = 'Lock'
             UNION ALL
               SELECT l.pid, false FROM pg_locks l
               WHERE $1::text IS NOT NULL AND l.granted AND l.locktype = 'relation'
                 AND l.relation = to_regclass($1::text) AND l.mode <> 'AccessShareLock'
           ), holders AS (
               SELECT pid, bool_or(blocking) AS blocking FROM found GROUP BY pid
           )
           SELECT a.pid AS "pid!",
                  h.blocking AS "blocking!",
                  a.state,
                  extract(epoch FROM clock_timestamp() - a.xact_start)::float8 AS transaction_seconds,
                  extract(epoch FROM clock_timestamp() - a.state_change)::float8 AS state_seconds,
                  a.wait_event_type || ':' || a.wait_event AS wait_event,
                  CASE WHEN a.application_name = $2
                       THEN left(regexp_replace(a.query, '\s+', ' ', 'g'), 160)
                       ELSE 'another client (statement not shown)' END AS query
           FROM pg_stat_activity a JOIN holders h ON h.pid = a.pid
           WHERE a.pid <> pg_backend_pid()
           ORDER BY h.blocking DESC, a.xact_start NULLS LAST
           LIMIT 3"#,
        relation,
        APPLICATION,
    )
    .fetch_all(pool)
    .await
}

/// One line naming the holders, for the log.
pub(crate) fn describe(holders: &[Holder]) -> String {
    if holders.is_empty() {
        return "no holder found (it may have ended)".to_owned();
    }
    holders
        .iter()
        .map(|holder| {
            format!(
                "pid {} {} ({}, transaction {:.1} s, state {:.1} s, waiting {}): {}",
                holder.pid,
                holder.state.as_deref().unwrap_or("unknown"),
                if holder.blocking {
                    "blocking a waiter"
                } else {
                    "holds a lock on the table"
                },
                holder.transaction_seconds.unwrap_or_default(),
                holder.state_seconds.unwrap_or_default(),
                holder.wait_event.as_deref().unwrap_or("no"),
                holder.query.as_deref().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The holder reads' gate: one in flight, one per [`HOLDER_READ_INTERVAL`], and a count of the lock timeouts that
/// got no read of their own.
pub(crate) struct HolderReads {
    state: Mutex<ReadState>,
}

struct ReadState {
    in_flight: bool,
    last: Option<Instant>,
    missed: u64,
}

impl HolderReads {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(ReadState {
                in_flight: false,
                last: None,
                missed: 0,
            }),
        }
    }

    /// A read may start at `now`: gives the lock timeouts since the last read that had none. `None` counts this one
    /// instead (a read is running, or the last began less than the interval ago).
    pub(crate) fn start(&self, now: Instant) -> Option<u64> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let recent = state
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < HOLDER_READ_INTERVAL);
        if state.in_flight || recent {
            state.missed = state.missed.saturating_add(1);
            return None;
        }
        state.in_flight = true;
        state.last = Some(now);
        Some(std::mem::take(&mut state.missed))
    }

    pub(crate) fn finish(&self) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .in_flight = false;
    }
}

/// Ends a read however its task ends.
struct ReadDone(&'static HolderReads);

impl Drop for ReadDone {
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// Logs a lock timeout with its row at once and, when this process has a pool to read from and no read is due
/// soon, its holders a moment later. Called from `From<sqlx::Error>`; the caller's error is unchanged
/// (`store_unavailable`).
pub(crate) fn report(error: &sqlx::Error) {
    let Some(timeout) = LockTimeout::of(error) else {
        return;
    };
    let context = timeout.context.as_deref().unwrap_or("no context");
    tracing::warn!(
        relation = timeout.relation.as_deref().unwrap_or("unknown"),
        "a database statement waited past lock_timeout: {context}"
    );
    let (Some(pool), Ok(runtime)) = (HOLDERS.get(), tokio::runtime::Handle::try_current()) else {
        return;
    };
    let Some(missed) = HOLDER_READS.start(Instant::now()) else {
        return;
    };
    let pool = pool.clone();
    // Made before the task: a task dropped before its first poll still ends the read, so reads never stop for good.
    let done = ReadDone(&HOLDER_READS);
    runtime.spawn(async move {
        let _done = done;
        let since = if missed == 0 {
            String::new()
        } else {
            format!(" ({missed} more lock timeouts since the last read)")
        };
        if let Ok(holders) = held_by(&pool, timeout.relation.as_deref()).await {
            tracing::warn!(
                relation = timeout.relation.as_deref().unwrap_or("unknown"),
                "the lock timeout's holder: {}{since}",
                describe(&holders)
            );
        } else {
            tracing::warn!("the lock timeout's holder couldn't be read{since}");
        }
    });
}

/// An advisory lock the gateway queues on: admissions, settings, the secret store, idempotency keys, bootstrap
/// and the action store.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Queue {
    Key(i64),
    /// Keyed by `hashtextextended(name, 0)`.
    Named(&'static str),
}

/// Takes an advisory transaction lock with no lock bound, as before the pool's: only the statement bound ends the
/// wait (57014, as before), since a queue is waited for on purpose and the last of several queued admissions on a
/// slow Postgres must still get in. The pool's lock bound applies again from the next statement in the transaction.
pub(crate) async fn queue(connection: &mut PgConnection, queue: Queue) -> Result<(), sqlx::Error> {
    sqlx::query!("SELECT set_config('lock_timeout', '0', true)")
        .fetch_one(&mut *connection)
        .await?;
    match queue {
        Queue::Key(key) => {
            sqlx::query!("SELECT pg_advisory_xact_lock($1)", key)
                .execute(&mut *connection)
                .await?
        }
        Queue::Named(name) => {
            sqlx::query!(
                "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
                name
            )
            .execute(&mut *connection)
            .await?
        }
    };
    sqlx::query!(
        "SELECT set_config('lock_timeout', reset_val, true) FROM pg_settings WHERE name = 'lock_timeout'"
    )
    .fetch_one(&mut *connection)
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "db_tests.rs"]
mod tests;
