use super::EventStore;
use crate::GatewayError;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tqa_types::{Event, SeqChecker, ServerFrame};
use uuid::Uuid;

struct Context {
    run_id: Option<String>,
    step: Option<i64>,
    timestamp: OffsetDateTime,
    /// The run lifecycle event type, recorded apart from the frame so blob-stored frames count too.
    lifecycle: Option<&'static str>,
    /// A `tool.call`'s call id, so the Git relay finds the exact approved call (065g).
    call_id: Option<String>,
}

impl EventStore {
    pub(super) async fn persist(
        &self,
        id: Uuid,
        sequence: i64,
        frame: &ServerFrame,
    ) -> Result<(), GatewayError> {
        let mut transaction = self.pool.begin().await?;
        let state =
            sqlx::query_scalar!("SELECT state FROM conversations WHERE id=$1 FOR UPDATE", id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(GatewayError::NotFound)?;
        if state != "active" {
            return Err(GatewayError::Conflict("conversation_deleted"));
        }
        let last = sqlx::query_scalar!("SELECT MAX(seq) FROM events WHERE conversation_id=$1", id)
            .fetch_one(&mut *transaction)
            .await?
            .unwrap_or(0);
        SeqChecker::new(crate::wire::from_db_sequence(last)?)
            .accept(crate::wire::from_db_sequence(sequence)?)?;
        let bytes =
            serde_json::to_vec(frame).map_err(|_| GatewayError::BadRequest("invalid frame"))?;
        let (inline, blob) = if bytes.len() > self.payload_cap || contains_nul(&bytes) {
            (
                None,
                Some(self.storage.write_blob(id, sequence, &bytes).await?),
            )
        } else {
            (
                Some(
                    serde_json::to_value(frame)
                        .map_err(|_| GatewayError::BadRequest("invalid frame"))?,
                ),
                None,
            )
        };
        let context = context(frame)?;
        sqlx::query!("INSERT INTO events (conversation_id,seq,run_id,step,ts,frame,blob_key,lifecycle,call_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            id, sequence, context.run_id, context.step, context.timestamp, inline, blob, context.lifecycle, context.call_id)
            .execute(&mut *transaction).await?;
        let team = self.team_frame(&mut transaction, id, frame).await?;
        // A run's placeholders are bound at its start and removed at its end, with the event (ADR-0037 §3); a resume
        // keeps the ones its start bound.
        match (context.lifecycle, context.run_id.as_deref()) {
            (Some("run.started"), Some(run)) => {
                crate::host_keys::bind_run(&mut transaction, id, run).await?;
            }
            (Some("run.completed" | "run.failed" | "run.cancelled"), Some(run)) => {
                crate::host_keys::end_run(&mut transaction, id, run).await?;
            }
            _ => {}
        }
        if matches!(
            frame,
            ServerFrame::Event {
                event: tqa_types::Event::RunCompleted { .. }
                    | tqa_types::Event::RunFailed { .. }
                    | tqa_types::Event::RunCancelled { .. },
                ..
            }
        ) {
            sqlx::query!(
                "UPDATE conversations SET last_activity_at=CURRENT_TIMESTAMP WHERE id=$1",
                id
            )
            .execute(&mut *transaction)
            .await?;
        }
        // A failed or cancelled commit may have reached PostgreSQL. Keep its immutable blob;
        // deleting it here could corrupt an acknowledged commit after a connection failure.
        transaction.commit().await?;
        if let Some(pending) = &self.team_reads {
            let (reads, ended) = team;
            pending.remove(id, &reads);
            if let Some(run) = &ended {
                pending.run_ended(id, run);
            }
        }
        Ok(())
    }

    /// One frame's agent-team work in the transaction that stores it: the reads its `tool.result` ends advance, and a
    /// run's end prunes that run's outstanding reads. Answers both, for the index after the commit; nothing, with no
    /// work, when agent teams are off or no read waits.
    async fn team_frame(
        &self,
        transaction: &mut sqlx::PgConnection,
        id: Uuid,
        frame: &ServerFrame,
    ) -> Result<(Vec<String>, Option<String>), GatewayError> {
        let reads = self.team_reads_ended(id, std::iter::once(frame));
        if !reads.is_empty() {
            crate::teams::store::result_stored(transaction, id, &reads).await?;
        }
        // A helper's state follows its runs, with the event that starts or ends one (agent teams on only).
        if let (Some(_), ServerFrame::Event { event, .. }) = (&self.team_reads, frame)
            && let Some(kind) = lifecycle(event)
        {
            crate::teams::member_state::on_lifecycle(transaction, id, kind).await?;
        }
        let ended = run_end(frame).filter(|run| {
            self.team_reads
                .as_ref()
                .is_some_and(|pending| pending.has_run(id, run))
        });
        if let Some(run) = &ended {
            crate::teams::store::run_ended(transaction, id, run).await?;
        }
        Ok((reads, ended))
    }
}

/// One batch's rows, column by column, in frame order: the arrays `UNNEST` reads back as rows.
#[derive(Default)]
struct Columns {
    seq: Vec<i64>,
    run_id: Vec<Option<String>>,
    step: Vec<Option<i64>>,
    ts: Vec<OffsetDateTime>,
    frame: Vec<Option<serde_json::Value>>,
    blob_key: Vec<Option<String>>,
    lifecycle: Vec<Option<String>>,
    call_id: Vec<Option<String>>,
}

impl EventStore {
    /// Stores consecutive plain journal frames in one transaction (GW-I-177): the conversation is locked and checked
    /// once, every sequence is checked in order against the stored one, and all rows go in one `INSERT` in frame
    /// order. Any refusal leaves nothing of the batch stored. Lifecycle frames, with their per-event effects, never
    /// come here.
    pub(super) async fn persist_batch(
        &self,
        id: Uuid,
        frames: &[(i64, ServerFrame)],
    ) -> Result<(), GatewayError> {
        let mut transaction = self.pool.begin().await?;
        let state =
            sqlx::query_scalar!("SELECT state FROM conversations WHERE id=$1 FOR UPDATE", id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(GatewayError::NotFound)?;
        if state != "active" {
            return Err(GatewayError::Conflict("conversation_deleted"));
        }
        let last = sqlx::query_scalar!("SELECT MAX(seq) FROM events WHERE conversation_id=$1", id)
            .fetch_one(&mut *transaction)
            .await?
            .unwrap_or(0);
        let mut checker = SeqChecker::new(crate::wire::from_db_sequence(last)?);
        for (sequence, _) in frames {
            checker.accept(crate::wire::from_db_sequence(*sequence)?)?;
        }
        let mut columns = Columns::default();
        for (sequence, frame) in frames {
            let bytes =
                serde_json::to_vec(frame).map_err(|_| GatewayError::BadRequest("invalid frame"))?;
            let (inline, blob) = if bytes.len() > self.payload_cap || contains_nul(&bytes) {
                (
                    None,
                    Some(self.storage.write_blob(id, *sequence, &bytes).await?),
                )
            } else {
                let value = serde_json::to_value(frame)
                    .map_err(|_| GatewayError::BadRequest("invalid frame"))?;
                (Some(value), None)
            };
            let context = context(frame)?;
            columns.seq.push(*sequence);
            columns.run_id.push(context.run_id);
            columns.step.push(context.step);
            columns.ts.push(context.timestamp);
            columns.frame.push(inline);
            columns.blob_key.push(blob);
            columns.lifecycle.push(context.lifecycle.map(str::to_owned));
            columns.call_id.push(context.call_id);
        }
        sqlx::query!(
            "INSERT INTO events (conversation_id,seq,run_id,step,ts,frame,blob_key,lifecycle,call_id)
             SELECT $1, row.seq, row.run_id, row.step, row.ts, row.frame, row.blob_key, row.lifecycle, row.call_id
             FROM UNNEST($2::bigint[], $3::text[], $4::bigint[], $5::timestamptz[], $6::jsonb[], $7::text[],
                         $8::text[], $9::text[])
                  WITH ORDINALITY AS row(seq, run_id, step, ts, frame, blob_key, lifecycle, call_id, position)
             ORDER BY row.position",
            id,
            &columns.seq,
            &columns.run_id as &[Option<String>],
            &columns.step as &[Option<i64>],
            &columns.ts,
            &columns.frame as &[Option<serde_json::Value>],
            &columns.blob_key as &[Option<String>],
            &columns.lifecycle as &[Option<String>],
            &columns.call_id as &[Option<String>],
        )
        .execute(&mut *transaction)
        .await?;
        let reads = self.team_reads_ended(id, frames.iter().map(|(_, frame)| frame));
        if !reads.is_empty() {
            crate::teams::store::result_stored(&mut transaction, id, &reads).await?;
        }
        // As for one frame: a commit that fails may have reached PostgreSQL, so its blobs stay.
        transaction.commit().await?;
        if let Some(pending) = &self.team_reads {
            pending.remove(id, &reads);
        }
        Ok(())
    }
}

impl EventStore {
    /// The keys of the agent-team reads whose `tool.result`s these frames carry, among those outstanding for
    /// `conversation`, decided in memory: none, with no work, when agent teams are off or no such read waits. The
    /// caller advances them in the transaction that stores the frames, so a read is delivered exactly once (ADR-0034,
    /// the slice S1 contract; GW-I-195), and takes them from the index after its commit.
    fn team_reads_ended<'a>(
        &self,
        conversation: Uuid,
        frames: impl Iterator<Item = &'a ServerFrame>,
    ) -> Vec<String> {
        let Some(pending) = &self.team_reads else {
            return Vec::new();
        };
        let keys: Vec<String> = frames
            .filter_map(|frame| match frame {
                ServerFrame::Event {
                    run_id: Some(run),
                    event: Event::ToolResult { call_id, .. },
                    ..
                } => Some(tqa_tools::builtin::channel::idempotency_key(
                    &run.to_string(),
                    &call_id.to_string(),
                )),
                _ => None,
            })
            .collect();
        if keys.is_empty() {
            return keys;
        }
        pending.outstanding(conversation, keys)
    }
}

/// The run a terminal lifecycle frame ends.
fn run_end(frame: &ServerFrame) -> Option<String> {
    match frame {
        ServerFrame::Event {
            run_id: Some(run),
            event: Event::RunCompleted { .. } | Event::RunFailed { .. } | Event::RunCancelled { .. },
            ..
        } => Some(run.to_string()),
        _ => None,
    }
}

/// `PostgreSQL` `jsonb` cannot hold U+0000, which serialized JSON always writes as the escape `\u0000`. Such frames
/// go to the blob store unchanged; an escaped backslash followed by `u0000` only matches harmlessly.
fn contains_nul(bytes: &[u8]) -> bool {
    bytes.windows(6).any(|window| window == br"\u0000")
}

fn context(frame: &ServerFrame) -> Result<Context, GatewayError> {
    if let ServerFrame::Event {
        run_id,
        step,
        ts,
        event,
        ..
    } = frame
    {
        Ok(Context {
            run_id: run_id.as_ref().map(ToString::to_string),
            step: step.map(i64::from),
            timestamp: OffsetDateTime::parse(ts, &Rfc3339)
                .map_err(|_| GatewayError::BadRequest("invalid event timestamp"))?,
            lifecycle: lifecycle(event),
            call_id: match event {
                Event::ToolCall { call_id, .. } => Some(call_id.to_string()),
                _ => None,
            },
        })
    } else {
        Ok(Context {
            run_id: None,
            step: None,
            timestamp: OffsetDateTime::now_utc(),
            lifecycle: None,
            call_id: None,
        })
    }
}

const fn lifecycle(event: &Event) -> Option<&'static str> {
    match event {
        Event::RunStarted { .. } => Some("run.started"),
        Event::RunResumed { .. } => Some("run.resumed"),
        Event::RunCompleted { .. } => Some("run.completed"),
        Event::RunFailed { .. } => Some("run.failed"),
        Event::RunCancelled { .. } => Some("run.cancelled"),
        _ => None,
    }
}
