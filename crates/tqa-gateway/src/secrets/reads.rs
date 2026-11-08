//! Secret read audit (ADR-0033; GW-U-055, GW-I-094, GW-I-095, GW-I-147). Every read of a
//! secret's value by the gateway (a model route's key, a connector credential, a host key) is recorded:
//! which consumer read which reference, for which conversation, user and run, and with what outcome. Never a value, a
//! hash or a length. Records go through a bounded queue to one writer that aggregates them into one row per key with a
//! count (a group commit). No key is used without its row (GW-I-147): a read that found a value waits for its batch to
//! commit before the value is returned, a full queue makes the read wait for room rather than dropping the record, and
//! a read whose row can't be written within the bound fails with `key_use_unrecorded`. Records and administration
//! audit rows are swept past their retention.
use crate::{GatewayError, app::State, secrets::SecretRef};
use secrecy::SecretString;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
use time::OffsetDateTime;
use tokio::sync::{mpsc, oneshot, watch};
use uuid::Uuid;

/// Records one write takes at most.
const BATCH: usize = 256;
/// How often retention runs.
const SWEEP: Duration = Duration::from_hours(1);
/// Rows one delete removes at most.
const SWEEP_BATCH: i64 = 1000;

/// What read a secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ConsumerKind {
    /// A model route's key, read by the relay (and the route probe).
    Route,
    /// A connector's credential, read by the MCP proxy.
    Connector,
    /// A decision engine's credential, read by the engine relay (ADR-0013 point 6).
    Engine,
    /// A host key's value, read by the proxy to swap it for its placeholder (ADR-0037 §8).
    Host,
}

impl ConsumerKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Route => "route",
            Self::Connector => "connector",
            Self::Engine => "engine",
            Self::Host => "host",
        }
    }
}

/// Whose read it was: the conversation, user and run in scope where the secret was read, as far as they are known.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ReadContext {
    pub conversation: Option<Uuid>,
    pub user: Option<Uuid>,
    pub run: Option<String>,
}

/// How a read ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Outcome {
    Ok,
    Absent,
    Unavailable,
    /// A host key's use the proxy refused, by its reason (ADR-0037 §8).
    Refused(&'static str),
}

impl Outcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Absent => "absent",
            Self::Unavailable => "unavailable",
            Self::Refused(reason) => reason,
        }
    }
}

/// The key one row aggregates.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    reference: String,
    kind: ConsumerKind,
    consumer: String,
    context: ReadContext,
    outcome: Outcome,
}

/// Reads of one key within a batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tally {
    first: OffsetDateTime,
    last: OffsetDateTime,
    count: i64,
}

enum Message {
    /// A read, and for a read whose value will be used, where to say whether its batch committed.
    Read(Key, OffsetDateTime, Option<oneshot::Sender<bool>>),
    Flush(oneshot::Sender<()>),
}

/// The queue between reads and the writer.
pub(crate) struct ReadAudit {
    sender: mpsc::Sender<Message>,
    receiver: Mutex<Option<mpsc::Receiver<Message>>>,
    hold: watch::Sender<bool>,
}

impl ReadAudit {
    pub(crate) fn new(capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity.max(1));
        Self {
            sender,
            receiver: Mutex::new(Some(receiver)),
            hold: watch::Sender::new(false),
        }
    }

    /// Holds the writer (true) or lets it run (false); for tests of a full queue.
    pub(crate) fn hold(&self, held: bool) {
        self.hold.send_replace(held);
    }

    /// Waits until everything queued before this call is written.
    ///
    /// # Errors
    /// The writer has stopped.
    pub(crate) async fn flush(&self) -> Result<(), GatewayError> {
        let (done, wait) = oneshot::channel();
        self.sender
            .send(Message::Flush(done))
            .await
            .map_err(|_| GatewayError::StoreUnavailable)?;
        wait.await.map_err(|_| GatewayError::StoreUnavailable)
    }
}

/// Reads `reference` for `consumer` and records the read. A value is returned only once its read's row is committed
/// (GW-I-147); a failed read is recorded too, without waiting for the commit.
///
/// # Errors
/// The store's failure, unchanged (it is recorded as `unavailable`); `key_use_unrecorded` when a value was found but
/// its row could not be written within the bound (the value is not used).
pub(crate) async fn read(
    state: &State,
    reference: &SecretRef,
    (kind, consumer): (ConsumerKind, &str),
    context: &ReadContext,
) -> Result<Option<SecretString>, GatewayError> {
    // A shared secret is read only for a consumer it is bound to; users' own credentials are only ever read for their
    // owner.
    let bound = match kind {
        // A host key's use is decided by the proxy's rules before its value is read (ADR-0037 §3).
        ConsumerKind::Host => true,
        ConsumerKind::Route | ConsumerKind::Connector | ConsumerKind::Engine => {
            tqa_types::is_per_user_secret(reference.as_str())
                || crate::secrets::bindings::allows(
                    &state.settings.current().bindings,
                    reference.as_str(),
                    kind.as_str(),
                    consumer,
                )
        }
    };
    let result = if bound {
        state.secret_store.get(reference).await
    } else {
        tracing::warn!(
            secret_ref = crate::secrets::loggable(reference.as_str()),
            consumer_kind = kind.as_str(),
            consumer,
            "a secret was not read: it is not bound to this consumer"
        );
        Err(GatewayError::SecretUnavailable)
    };
    let outcome = match &result {
        Ok(Some(_)) => Outcome::Ok,
        Ok(None) => Outcome::Absent,
        Err(_) => Outcome::Unavailable,
    };
    let key = Key {
        reference: reference.as_str().to_owned(),
        kind,
        consumer: consumer.to_owned(),
        context: context.clone(),
        outcome,
    };
    let used = matches!(result, Ok(Some(_)));
    if !recorded(state, key, used).await {
        state.telemetry.secret_read_unrecorded();
        if used {
            tracing::warn!(
                secret_ref = crate::secrets::loggable(reference.as_str()),
                consumer_kind = kind.as_str(),
                consumer,
                "a key was not used: its use could not be recorded"
            );
            return Err(GatewayError::KeyUseUnrecorded);
        }
    }
    result
}

/// Records a host key use the proxy refused (`outcome`, one of the host outcomes), without reading the value or
/// waiting for the commit: nothing is sent either way. Whether it was queued.
pub(crate) async fn refused(
    state: &State,
    reference: &SecretRef,
    consumer: &str,
    context: &ReadContext,
    outcome: &'static str,
) -> bool {
    let key = Key {
        reference: reference.as_str().to_owned(),
        kind: ConsumerKind::Host,
        consumer: consumer.to_owned(),
        context: context.clone(),
        outcome: Outcome::Refused(outcome),
    };
    let queued = recorded(state, key, false).await;
    if !queued {
        state.telemetry.secret_read_unrecorded();
    }
    queued
}

/// Queues the read's record, waiting for room (never dropping it), and for a read whose value will be `used`, waits for
/// its batch to commit; both within the I/O bound. Whether it was recorded.
async fn recorded(state: &State, key: Key, used: bool) -> bool {
    let bound = state.config.io_timeout();
    let (ack, committed) = if used {
        let (ack, committed) = oneshot::channel();
        (Some(ack), Some(committed))
    } else {
        (None, None)
    };
    let queued = tokio::time::timeout(
        bound,
        state
            .read_audit
            .sender
            .send(Message::Read(key, OffsetDateTime::now_utc(), ack)),
    )
    .await;
    match (queued, committed) {
        (Ok(Ok(())), Some(committed)) => {
            matches!(tokio::time::timeout(bound, committed).await, Ok(Ok(true)))
        }
        (Ok(Ok(())), None) => true,
        _ => false,
    }
}

/// Adds one read to a batch's tallies.
fn tally(tallies: &mut HashMap<Key, Tally>, key: Key, at: OffsetDateTime) {
    tallies
        .entry(key)
        .and_modify(|tally| {
            tally.first = tally.first.min(at);
            tally.last = tally.last.max(at);
            tally.count += 1;
        })
        .or_insert(Tally {
            first: at,
            last: at,
            count: 1,
        });
}

async fn write(state: &State, tallies: HashMap<Key, Tally>) -> Result<(), GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        for (key, tally) in tallies {
            sqlx::query!(
                "INSERT INTO secret_reads (id,ref,consumer_kind,consumer,conversation_id,user_id,run_id,outcome,first_at,last_at,count) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
                 ON CONFLICT (ref,consumer_kind,consumer,conversation_id,user_id,run_id,outcome) DO UPDATE SET \
                 count=secret_reads.count+EXCLUDED.count, last_at=GREATEST(secret_reads.last_at,EXCLUDED.last_at)",
                Uuid::now_v7(),
                key.reference,
                key.kind.as_str(),
                key.consumer,
                key.context.conversation,
                key.context.user,
                key.context.run,
                key.outcome.as_str(),
                tally.first,
                tally.last,
                tally.count
            )
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    })
    .await
}

/// Starts the writer, and the hourly retention sweep.
pub(crate) fn start(state: &Arc<State>) {
    let receiver = state
        .read_audit
        .receiver
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(receiver) = receiver {
        state.tasks.spawn(writer(state.clone(), receiver));
    }
    let owned = state.clone();
    state.tasks.spawn(async move {
        let mut interval = tokio::time::interval(SWEEP);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {biased;
                () = owned.cancel.cancelled() => break,
                _ = interval.tick() => {
                    if sweep(&owned).await.is_err() && !owned.cancel.is_cancelled() {
                        tracing::warn!("audit retention sweep failed");
                    }
                    // A run's placeholders, a day after it ended (ADR-0037 §3).
                    if crate::host_keys::sweep_ended(&owned).await.is_err() && !owned.cancel.is_cancelled() {
                        tracing::warn!("host key placeholder sweep failed");
                    }
                },
            }
        }
    });
}

async fn writer(state: Arc<State>, mut receiver: mpsc::Receiver<Message>) {
    let mut hold = state.read_audit.hold.subscribe();
    loop {
        // Held (tests of a full queue): nothing is taken off the queue.
        if hold.wait_for(|held| !held).await.is_err() {
            return;
        }
        let first = tokio::select! {biased;
            () = state.cancel.cancelled() => return,
            message = receiver.recv() => match message { Some(message) => message, None => return },
        };
        let mut tallies = HashMap::new();
        let mut flushes = Vec::new();
        let mut acks = Vec::new();
        let mut next = Some(first);
        while let Some(message) = next.take() {
            match message {
                Message::Read(key, at, ack) => {
                    tally(&mut tallies, key, at);
                    acks.extend(ack);
                }
                Message::Flush(done) => flushes.push(done),
            }
            if tallies.len() < BATCH {
                next = receiver.try_recv().ok();
            }
        }
        let committed = tallies.is_empty() || write(&state, tallies).await.is_ok();
        if !committed && !state.cancel.is_cancelled() {
            tracing::warn!("secret read records could not be written");
        }
        // Each waiting read learns whether its row is committed; a read no longer waiting has given up already.
        for ack in acks {
            let _ = ack.send(committed);
        }
        for done in flushes {
            let _ = done.send(());
        }
    }
}

/// Deletes read records past `read_audit_days` and administration records past `admin_audit_days`, in batches.
/// Returns how many of each it deleted.
///
/// # Errors
/// A store failure.
pub(crate) async fn sweep(state: &State) -> Result<(u64, u64), GatewayError> {
    let config = &state.config.secret_store;
    let (reads_days, audit_days) = (
        i32::try_from(config.read_audit_days).unwrap_or(i32::MAX),
        i32::try_from(config.admin_audit_days).unwrap_or(i32::MAX),
    );
    let mut reads = 0;
    loop {
        let deleted = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            Ok(sqlx::query!(
                "DELETE FROM secret_reads WHERE id IN (SELECT id FROM secret_reads \
                 WHERE last_at < CURRENT_TIMESTAMP - make_interval(days => $1) LIMIT $2)",
                reads_days,
                SWEEP_BATCH
            )
            .execute(&state.pool)
            .await?
            .rows_affected())
        })
        .await?;
        reads += deleted;
        if deleted == 0 {
            break;
        }
    }
    let mut audit = 0;
    loop {
        let deleted = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            Ok(sqlx::query!(
                "DELETE FROM admin_audit WHERE id IN (SELECT id FROM admin_audit \
                 WHERE at < CURRENT_TIMESTAMP - make_interval(days => $1) LIMIT $2)",
                audit_days,
                SWEEP_BATCH
            )
            .execute(&state.pool)
            .await?
            .rows_affected())
        })
        .await?;
        audit += deleted;
        if deleted == 0 {
            break;
        }
    }
    if reads + audit > 0 {
        tracing::info!(reads, audit, "audit retention swept old records");
    }
    Ok((reads, audit))
}

/// One read record as the APIs show it: never a value. `user_id` only in the administrator's view.
#[derive(serde::Serialize)]
pub(crate) struct ReadItem {
    id: Uuid,
    consumer_kind: String,
    consumer: String,
    conversation_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_id: Option<Uuid>,
    run_id: Option<String>,
    outcome: String,
    count: i64,
    #[serde(with = "time::serde::rfc3339")]
    first_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    last_at: OffsetDateTime,
}

/// A page of read records, newest first; `next` is the `before` of the following page, if there is one.
#[derive(serde::Serialize)]
pub(crate) struct ReadPage {
    items: Vec<ReadItem>,
    next: Option<Uuid>,
}

/// A read page's parameters: how many, and after which record.
pub(crate) struct Paging {
    pub limit: i64,
    pub before: Option<Uuid>,
}

/// The paging parameters `?limit=1..=100&before=<id>`; `limit` defaults to 50.
///
/// # Errors
/// `400 invalid_request` for an unknown, repeated or invalid parameter.
pub(crate) fn paging(query: Option<&str>) -> Result<Paging, GatewayError> {
    let invalid = || GatewayError::Invalid {
        field: "query",
        reason: crate::InvalidReason::ParameterInvalid,
    };
    let (mut limit, mut before) = (None, None);
    for (key, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "limit" if limit.is_none() => {
                limit = Some(
                    value
                        .parse::<i64>()
                        .ok()
                        .filter(|limit| (1..=100).contains(limit))
                        .ok_or_else(invalid)?,
                );
            }
            "before" if before.is_none() => {
                before = Some(value.parse::<Uuid>().map_err(|_| invalid())?);
            }
            _ => return Err(invalid()),
        }
    }
    Ok(Paging {
        limit: limit.unwrap_or(50),
        before,
    })
}

/// The read records of `reference`, newest first; with `with_user`, each names the user it was read for.
///
/// # Errors
/// A store failure.
pub(crate) async fn page(
    state: &State,
    reference: &SecretRef,
    paging: &Paging,
    with_user: bool,
) -> Result<ReadPage, GatewayError> {
    page_of(state, reference, paging, with_user, None).await
}

/// [`page`], of one user's reads only when `only_user` names them (a member's view of a host key shared with them, ADR-0037 §8).
///
/// # Errors
/// A store failure.
pub(crate) async fn page_of(
    state: &State,
    reference: &SecretRef,
    paging: &Paging,
    with_user: bool,
    only_user: Option<Uuid>,
) -> Result<ReadPage, GatewayError> {
    let (limit, before) = (paging.limit, paging.before);
    let rows = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT id,consumer_kind,consumer,conversation_id,user_id,run_id,outcome,count,first_at,last_at FROM secret_reads \
             WHERE ref=$1 AND ($2::uuid IS NULL OR (last_at,id) < (SELECT last_at,id FROM secret_reads WHERE id=$2)) \
             AND ($4::uuid IS NULL OR user_id=$4) ORDER BY last_at DESC, id DESC LIMIT $3",
            reference.as_str(),
            before,
            limit + 1,
            only_user
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    let more = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
    let items: Vec<ReadItem> = rows
        .into_iter()
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .map(|row| ReadItem {
            id: row.id,
            consumer_kind: row.consumer_kind,
            consumer: row.consumer,
            conversation_id: row.conversation_id,
            user_id: row.user_id.filter(|_| with_user),
            run_id: row.run_id,
            outcome: row.outcome,
            count: row.count,
            first_at: row.first_at,
            last_at: row.last_at,
        })
        .collect();
    let next = more.then(|| items.last().map(|item| item.id)).flatten();
    Ok(ReadPage { items, next })
}

#[cfg(test)]
#[path = "reads_tests.rs"]
mod tests;
