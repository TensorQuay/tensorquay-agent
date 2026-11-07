//! Test-only WAL adapter: a single owned writer and acknowledgements after append/sync.
use async_trait::async_trait;
use std::{error::Error, sync::Arc, time::Duration};
use tokio::{
    runtime::Runtime,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tqa_context::{Wal, WalError, wal_should_sync};
use tqa_core::{EventContext, EventSink, SinkError};
use tqa_types::{ErrorCode, Event, Seq, ServerFrame};

pub(super) struct Journal {
    pub sink: Arc<JournalSink>,
    worker: JoinHandle<Result<Vec<Seq>, WalError>>,
    directory: tempfile::TempDir,
}

pub(super) struct Snapshot {
    pub frames: Vec<ServerFrame>,
    pub synced: Vec<Seq>,
}

impl Journal {
    pub(super) fn new(runtime: &Runtime) -> Result<Self, Box<dyn Error>> {
        // Private state is separate from the fake tool's workspace.
        let directory = tempfile::tempdir()?;
        let (wal, recovery) = Wal::open(directory.path())?;
        assert!(recovery.records.is_empty());
        let (sender, receiver) = mpsc::channel(1);
        let worker = runtime.spawn_blocking(move || write_events(wal, receiver));
        Ok(Self {
            sink: Arc::new(JournalSink { sender }),
            worker,
            directory,
        })
    }
    pub(super) fn finish(self, runtime: &Runtime) -> Result<Snapshot, Box<dyn Error>> {
        drop(self.sink);
        let synced = runtime.block_on(self.worker)??;
        let (_wal, recovery) = Wal::open(self.directory.path())?;
        assert!(
            !recovery.truncated_tail,
            "acknowledged records must be complete"
        );
        let frames = recovery
            .records
            .into_iter()
            .map(|(_, frame)| frame)
            .collect();
        Ok(Snapshot { frames, synced })
    }
}

pub(super) struct JournalSink {
    sender: mpsc::Sender<Append>,
}

struct Append {
    event: Event,
    context: EventContext,
    reply: oneshot::Sender<Seq>,
}

#[async_trait]
impl EventSink for JournalSink {
    async fn emit(&self, event: Event, context: EventContext) -> Result<Seq, SinkError> {
        let append = async {
            let (reply, result) = oneshot::channel();
            self.sender
                .send(Append {
                    event,
                    context,
                    reply,
                })
                .await
                .map_err(|_| sink_error())?;
            result.await.map_err(|_| sink_error())
        };
        tokio::time::timeout(Duration::from_secs(30), append)
            .await
            .map_err(|_| sink_error())?
    }
}

fn sink_error() -> SinkError {
    SinkError {
        code: ErrorCode::Io,
    }
}

fn write_events(mut wal: Wal, mut receiver: mpsc::Receiver<Append>) -> Result<Vec<Seq>, WalError> {
    let mut seq = Seq::ZERO;
    let mut unsynced = 0;
    let mut synced = Vec::new();
    while let Some(append) = receiver.blocking_recv() {
        let sync = wal_should_sync(&append.event, unsynced + 1);
        seq = seq
            .next()
            .ok_or_else(|| std::io::Error::other("benchmark sequence exhausted"))?;
        let frame = ServerFrame::Event {
            seq,
            run_id: append.context.run_id,
            step: append.context.step,
            ts: "2001-01-01T00:00:00Z".into(),
            event: append.event,
        };
        wal.append(&frame)?;
        unsynced += 1;
        if sync {
            wal.sync()?;
            synced.push(seq);
            unsynced = 0;
        }
        if append.reply.send(seq).is_err() {
            break;
        }
    }
    Ok(synced)
}
