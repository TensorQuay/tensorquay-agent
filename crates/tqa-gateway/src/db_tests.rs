//! GW-U-209: the pool's connections bound lock waits and idle transactions, the migration connection doesn't, and
//! the pool's idle and lifetime times are written down. GW-U-210: at most one holder read at a time and one per
//! interval, with the timeouts in between counted. GW-I-316: a row held by another session ends a pooled
//! statement's wait with SQLSTATE 55P03 within the lock bound, and the holder is named, its statement only if it
//! is ours (16z's PERF-011). GW-I-317: an advisory queue keeps the statement bound while a row keeps the lock bound.
use super::{
    APPLICATION, HOLDER_READ_INTERVAL, HolderReads, LockTimeout, POOL_IDLE, POOL_LIFETIME, Queue,
    base_options, describe, held_by, idle_in_transaction_timeout, lock_timeout, pool_options,
    pooled_options, queue, relation_of,
};
use crate::{GatewayConfig, GatewayError, GatewaySecrets};
use secrecy::SecretString;
use sqlx::{
    Connection, PgConnection, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The default I/O bound, `[limits] io_timeout = 10`.
const IO: Duration = Duration::from_secs(10);

// TEST: GW-U-209
#[test]
fn gw_u_209_pool_connections_bound_lock_waits_and_idle_transactions() -> TestResult {
    assert_eq!(
        lock_timeout(IO),
        Duration::from_secs(5),
        "half the statement bound"
    );
    assert_eq!(
        lock_timeout(Duration::from_secs(1)),
        Duration::from_millis(500)
    );
    assert_eq!(idle_in_transaction_timeout(IO), Duration::from_secs(30));
    let base = PgConnectOptions::new().options([("statement_timeout", "10000")]);
    let pooled = pooled_options(base.clone(), IO);
    let options = pooled
        .get_options()
        .ok_or("the pool's connections have startup options")?;
    assert!(options.contains("-c lock_timeout=5000"), "{options}");
    assert!(
        options.contains("-c idle_in_transaction_session_timeout=30000"),
        "{options}"
    );
    assert!(
        options.contains("-c statement_timeout=10000"),
        "the base settings stay: {options}"
    );
    let migration = base.get_options().unwrap_or_default();
    assert!(
        !migration.contains("lock_timeout"),
        "migrations may wait on purpose: {migration}"
    );
    assert!(!migration.contains("idle_in_transaction"), "{migration}");
    let config = GatewayConfig::from_toml(
        "listen='127.0.0.1:0'\npublic_origin='https://gateway.example'\nmodels_path='models.toml'\n\
         [storage]\nroot='/gateway-storage'\n",
    )?;
    // Every connection, the migration one too, carries the gateway's application name.
    let secrets = GatewaySecrets::new(
        SecretString::from("postgres://gateway@db.invalid/tqa"),
        None,
    )?;
    let gateway = base_options(&config, &secrets)?;
    assert_eq!(gateway.get_application_name(), Some(APPLICATION));
    let settings = gateway.get_options().unwrap_or_default();
    assert!(
        settings.contains("-c statement_timeout=10000") && !settings.contains("lock_timeout"),
        "{settings}"
    );
    let pool = pool_options(&config);
    assert_eq!(pool.get_idle_timeout(), Some(POOL_IDLE));
    assert_eq!(pool.get_max_lifetime(), Some(POOL_LIFETIME));
    assert_eq!((POOL_IDLE.as_secs(), POOL_LIFETIME.as_secs()), (600, 1800));
    assert_eq!(
        relation_of(r#"while updating tuple (0,5) in relation "workspaces""#).as_deref(),
        Some("workspaces")
    );
    assert_eq!(
        relation_of("SQL statement \"SELECT pg_advisory_xact_lock(1)\""),
        None
    );
    Ok(())
}

// TEST: GW-U-210
#[test]
fn gw_u_210_one_holder_read_at_a_time_and_the_rest_counted() {
    let reads = HolderReads::new();
    let start = Instant::now();
    let burst: Vec<Option<u64>> = (0..20).map(|_| reads.start(start)).collect();
    assert_eq!(
        burst.iter().filter(|r| r.is_some()).count(),
        1,
        "a burst of 20 timeouts gives one read"
    );
    assert_eq!(burst[0], Some(0));
    // Still running past the interval: no second read.
    assert_eq!(reads.start(start + HOLDER_READ_INTERVAL * 2), None);
    reads.finish();
    // Done, but the last began less than the interval ago.
    let later = start + HOLDER_READ_INTERVAL * 2;
    assert_eq!(
        reads.start(later),
        Some(20),
        "the 20 timeouts with no read of their own"
    );
    reads.finish();
    assert_eq!(reads.start(later + Duration::from_secs(1)), None);
    assert_eq!(reads.start(later + HOLDER_READ_INTERVAL), Some(1));
}

// TEST: GW-I-316
#[sqlx::test(migrations = false)]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_316_a_held_row_is_named_within_the_lock_bound(pool: PgPool) -> TestResult {
    sqlx::query("CREATE TABLE held (id int PRIMARY KEY, n int NOT NULL)")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO held VALUES (1, 0), (2, 0)")
        .execute(&pool)
        .await?;
    let base = (*pool.connect_options()).clone();
    // The holder: a gateway session that changed the row and then sent nothing, as a transaction outside its bound
    // would.
    let mut holder =
        PgConnection::connect_with(&base.clone().application_name(APPLICATION)).await?;
    let holder_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut holder)
        .await?;
    sqlx::query("BEGIN").execute(&mut holder).await?;
    sqlx::query("UPDATE held SET n = n + 1 WHERE id = 1")
        .execute(&mut holder)
        .await?;
    // Another client under the same role holds row 2 with a literal in its statement.
    let mut other = PgConnection::connect_with(&base.clone().application_name("psql")).await?;
    let other_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut other)
        .await?;
    sqlx::query("BEGIN").execute(&mut other).await?;
    sqlx::query("UPDATE held SET n = 4242 WHERE id = 2")
        .execute(&mut other)
        .await?;

    let gateway = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(pooled_options(base.clone(), IO))
        .await?;
    let started = Instant::now();
    // Without the lock bound this waits until the holder ends: the client-side limit turns that into a failure.
    let (waited, _) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(
            sqlx::query("UPDATE held SET n = n + 1 WHERE id = 1").execute(&gateway),
            sqlx::query("UPDATE held SET n = n + 1 WHERE id = 2").execute(&gateway),
        )
    })
    .await
    .map_err(|_| "no lock timeout within 15 s: the pool's connections don't bound lock waits")?;
    let elapsed = started.elapsed();
    let error = waited.err().ok_or("the update went through a held row")?;
    let timeout =
        LockTimeout::of(&error).ok_or_else(|| format!("not a lock timeout (55P03): {error}"))?;
    assert_eq!(timeout.relation.as_deref(), Some("held"), "{timeout:?}");
    assert!(
        elapsed >= Duration::from_secs(4) && elapsed < IO,
        "after the 5 s lock bound and before the 10 s statement bound: {elapsed:?}"
    );

    let holders = held_by(&gateway, timeout.relation.as_deref()).await?;
    let named = holders
        .iter()
        .find(|h| h.pid == holder_pid)
        .ok_or_else(|| {
            format!(
                "the holder {holder_pid} isn't named: {}",
                describe(&holders)
            )
        })?;
    assert_eq!(named.state.as_deref(), Some("idle in transaction"));
    // Read after the waits were cut: it holds a lock on the table, which isn't proof it held the row.
    assert!(!named.blocking, "{named:?}");
    assert!(describe(&holders).contains(&format!(
        "pid {holder_pid} idle in transaction (holds a lock on the table"
    )));
    assert!(
        named.transaction_seconds.unwrap_or_default() >= 4.0,
        "{named:?}"
    );
    assert!(describe(&holders).contains(&format!("pid {holder_pid} idle in transaction")));
    assert!(
        named
            .query
            .as_deref()
            .is_some_and(|q| q.contains("UPDATE held")),
        "our own session's statement is shown: {named:?}"
    );
    let stranger = holders.iter().find(|h| h.pid == other_pid).ok_or_else(|| {
        format!(
            "the other client {other_pid} isn't named: {}",
            describe(&holders)
        )
    })?;
    assert_eq!(
        stranger.query.as_deref(),
        Some("another client (statement not shown)"),
        "another client's statement may hold literals"
    );
    assert!(!describe(&holders).contains("4242"));
    assert_eq!(
        GatewayError::from(error).code(),
        "store_unavailable",
        "the caller's error is unchanged"
    );

    // The migration connection has no lock bound; the pool's has both.
    let mut migration = PgConnection::connect_with(&base).await?;
    let unbounded: String = sqlx::query_scalar("SHOW lock_timeout")
        .fetch_one(&mut migration)
        .await?;
    assert_eq!(unbounded, "0");
    let lock: String = sqlx::query_scalar("SHOW lock_timeout")
        .fetch_one(&gateway)
        .await?;
    let idle: String = sqlx::query_scalar("SHOW idle_in_transaction_session_timeout")
        .fetch_one(&gateway)
        .await?;
    assert_eq!((lock.as_str(), idle.as_str()), ("5s", "30s"));
    sqlx::query("ROLLBACK").execute(&mut holder).await?;
    sqlx::query("ROLLBACK").execute(&mut other).await?;
    Ok(())
}

/// A session that holds what `statement` takes for 7 s, then commits.
async fn holds_for_7_s(
    options: &PgConnectOptions,
    statement: &'static str,
) -> Result<tokio::task::JoinHandle<Result<(), sqlx::Error>>, sqlx::Error> {
    let mut connection = PgConnection::connect_with(options).await?;
    sqlx::query("BEGIN").execute(&mut connection).await?;
    sqlx::query(statement).execute(&mut connection).await?;
    Ok(tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(7)).await;
        sqlx::query("COMMIT").execute(&mut connection).await?;
        Ok(())
    }))
}

// TEST: GW-I-317
#[sqlx::test(migrations = false)]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_317_an_advisory_queue_keeps_the_statement_bound(pool: PgPool) -> TestResult {
    sqlx::query("CREATE TABLE held (id int PRIMARY KEY, n int NOT NULL)")
        .execute(&pool)
        .await?;
    sqlx::query("INSERT INTO held VALUES (1, 0)")
        .execute(&pool)
        .await?;
    // The gateway's own settings: the 10 s statement bound, and the pool's 5 s lock bound.
    let base = (*pool.connect_options())
        .clone()
        .options([("statement_timeout", "10000")]);
    let gateway = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(pooled_options(base.clone(), IO))
        .await?;
    let queue_holder = holds_for_7_s(&base, "SELECT pg_advisory_xact_lock(74271012)").await?;
    let row_holder = holds_for_7_s(&base, "UPDATE held SET n = n + 1 WHERE id = 1").await?;
    let started = Instant::now();
    let queued = async {
        let mut transaction = gateway.begin().await?;
        queue(&mut transaction, Queue::Key(74_271_012)).await?;
        let waited = started.elapsed();
        // The pool's lock bound is back for the rest of the transaction.
        let lock: String = sqlx::query_scalar("SHOW lock_timeout")
            .fetch_one(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok::<_, sqlx::Error>((waited, lock))
    };
    let row = async {
        let outcome = sqlx::query("UPDATE held SET n = n + 1 WHERE id = 1")
            .execute(&gateway)
            .await;
        (outcome, started.elapsed())
    };
    // While both still wait, the row's holder is read as a real blocker.
    let read_while_waiting = async {
        tokio::time::sleep(Duration::from_secs(2)).await;
        held_by(&pool, Some("held")).await
    };
    let (queued, (row, row_waited), waiting) = tokio::join!(queued, row, read_while_waiting);
    let waiting = waiting?;
    assert!(
        waiting.iter().any(|holder| holder.blocking),
        "{}",
        describe(&waiting)
    );
    assert!(describe(&waiting).contains("blocking a waiter"));
    let (waited, lock) =
        queued.map_err(|error| format!("the queued admission didn't get in: {error}"))?;
    assert!(
        waited >= Duration::from_secs(6) && waited < IO,
        "it waited for the 7 s holder, past the 5 s lock bound: {waited:?}"
    );
    assert_eq!(lock, "5s");
    let error = row.err().ok_or("the row waiter went through a held row")?;
    let timeout =
        LockTimeout::of(&error).ok_or_else(|| format!("not a lock timeout (55P03): {error}"))?;
    assert_eq!(timeout.relation.as_deref(), Some("held"));
    assert!(
        row_waited < Duration::from_secs(7),
        "cut at the lock bound: {row_waited:?}"
    );
    queue_holder.await??;
    row_holder.await??;
    Ok(())
}
