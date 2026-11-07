//! Blocking storage ownership; no async task holds a file or lock across an await.
use super::{Command, JournalOptions, Records, Reply};
use crate::{IoCause, RuntimeError};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tqa_context::{Wal, wal_should_sync};
use tqa_core::EventContext;
use tqa_types::{Event, Seq, ServerFrame};

pub(super) fn run(
    options: JournalOptions,
    mut commands: mpsc::Receiver<Command>,
    frames: mpsc::Sender<ServerFrame>,
    changes: watch::Sender<Option<Arc<ServerFrame>>>,
    ready: Reply<()>,
) {
    let Ok((wal, recovery)) = Wal::open(&options.directory) else {
        let _ = ready.send(Err(RuntimeError::io("wal open")));
        return;
    };
    if recovery.truncated_tail {
        tracing::warn!("recovered an incomplete final journal record");
    }
    let last = recovery.records.last().map_or(Seq::ZERO, |(seq, _)| *seq);
    let mut worker = Worker {
        wal,
        last,
        registered: false,
        unsynced: 0,
        options,
        frames,
        changes,
    };
    if ready.send(Ok(())).is_err() {
        return;
    }
    while let Some(command) = commands.blocking_recv() {
        match command {
            Command::Append {
                event,
                context,
                reply,
            } => {
                let _ = reply.send(worker.append(*event, context));
            }
            Command::Register { watermark, reply } => {
                let _ = reply.send(worker.register(watermark));
            }
            Command::Records { after, reply } => {
                let _ = reply.send(Ok(worker.records(after)));
            }
            Command::Sync(reply) => {
                let _ = reply.send(worker.sync());
            }
            Command::Stop(reply) => {
                let result = worker.sync();
                drop(worker);
                let _ = reply.send(result);
                return;
            }
        }
    }
}
struct Worker {
    wal: Wal,
    last: Seq,
    registered: bool,
    unsynced: usize,
    options: JournalOptions,
    frames: mpsc::Sender<ServerFrame>,
    changes: watch::Sender<Option<Arc<ServerFrame>>>,
}
impl Worker {
    fn register(&mut self, watermark: Seq) -> Result<Records, RuntimeError> {
        if self.registered && watermark > self.last {
            return Err(RuntimeError::History);
        }
        self.wal
            .truncate_through(watermark)
            .map_err(|error| RuntimeError::Io(IoCause::wal("wal truncate", &error)))?;
        self.last = self.last.max(watermark);
        self.registered = true;
        self.unsynced = 0;
        Ok(self.records(watermark))
    }
    fn records(&self, after: Seq) -> Records {
        let records = self
            .wal
            .records_page(after, self.options.limits.queue_capacity);
        #[cfg(feature = "testing")]
        self.options
            .testing
            .replay_page_peak
            .fetch_max(records.len(), std::sync::atomic::Ordering::SeqCst);
        records
    }
    fn append(&mut self, event: Event, context: EventContext) -> Result<Seq, RuntimeError> {
        let seq = self.next_sequence()?;
        let sync = wal_should_sync(&event, self.unsynced + 1);
        let frame = ServerFrame::Event {
            seq,
            run_id: context.run_id,
            step: context.step,
            ts: self.options.wall_clock.timestamp()?,
            event,
        };
        self.persist(frame, seq, sync)
    }
    fn next_sequence(&self) -> Result<Seq, RuntimeError> {
        if !self.registered {
            return Err(RuntimeError::io("append before registration"));
        }
        self.last.next().ok_or(RuntimeError::History)
    }
    fn persist(&mut self, frame: ServerFrame, seq: Seq, sync: bool) -> Result<Seq, RuntimeError> {
        let bytes = serde_json::to_vec(&frame).map_err(|_| RuntimeError::Frame)?;
        ServerFrame::parse(&bytes).map_err(|_| RuntimeError::Frame)?;
        #[cfg(feature = "testing")]
        {
            self.options.testing.before_append.checkpoint_blocking();
            if self
                .options
                .testing
                .fail_appends
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return Err(RuntimeError::io("wal append (test)"));
            }
        }
        self.wal
            .append(&frame)
            .map_err(|error| RuntimeError::Io(IoCause::wal("wal append", &error)))?;
        self.last = seq;
        self.unsynced += 1;
        if sync {
            self.sync()?;
        }
        // This one-slot notification can coalesce: the WAL retains every omitted sequence.
        self.changes.send_replace(Some(Arc::new(frame.clone())));
        self.frames
            .blocking_send(frame)
            .map_err(|_| RuntimeError::io("link frames closed"))?;
        Ok(seq)
    }
    fn sync(&mut self) -> Result<(), RuntimeError> {
        #[cfg(feature = "testing")]
        if self
            .options
            .testing
            .fail_syncs
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(RuntimeError::io("wal sync (test)"));
        }
        self.wal
            .sync()
            .map_err(|error| RuntimeError::Io(IoCause::wal("wal sync", &error)))?;
        self.unsynced = 0;
        Ok(())
    }
}
