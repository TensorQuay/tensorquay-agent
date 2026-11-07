//! One owned blocking worker is the sole WAL and sequence writer.
mod worker;
use crate::{IoCause, RuntimeError, RuntimeLimits, WallClock};
use async_trait::async_trait;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};
use tqa_core::{EventContext, EventSink, SinkError};
use tqa_types::{Event, Seq, ServerFrame, Sleeper};

type Records = Vec<(Seq, ServerFrame)>;
type Reply<T> = oneshot::Sender<Result<T, RuntimeError>>;
enum Command {
    Append {
        /// Boxed: an event is far larger than every other command (clippy's `large_enum_variant`).
        event: Box<Event>,
        context: EventContext,
        reply: Reply<Seq>,
    },
    Register {
        watermark: Seq,
        reply: Reply<Records>,
    },
    Records {
        after: Seq,
        reply: Reply<Records>,
    },
    Sync(Reply<()>),
    Stop(Reply<()>),
}
/// Explicit resources for the blocking journal worker.
pub struct JournalOptions {
    /// Private state directory outside the workspace.
    pub directory: PathBuf,
    /// UTC timestamp source for stored frames.
    pub wall_clock: Arc<dyn WallClock>,
    /// Deadline source for queue and operation acknowledgements.
    pub sleeper: Arc<dyn Sleeper>,
    /// Queue and acknowledgement bounds.
    pub limits: RuntimeLimits,
    /// Flushes the workspace before each tool result is journaled, so a journaled result never runs ahead of its file
    /// changes. Set on the microVM backend (`sync_before_result`), whose guest page cache a VM crash loses.
    pub workspace_sync: Option<Arc<dyn WorkspaceSync>>,
    /// Non-default fault injection, absent from the shipped binary.
    #[cfg(feature = "testing")]
    pub testing: crate::testing::TestingHooks,
}
/// Makes the workspace's pending writes durable. Called from the journal's blocking worker.
pub trait WorkspaceSync: Send + Sync {
    /// Flushes the workspace's filesystem.
    ///
    /// # Errors
    /// `Io` when the flush fails; the tool result is then not journaled.
    fn sync(&self) -> Result<(), RuntimeError>;
}

/// `syncfs(2)` on the workspace mount: the guest's own ext4 filesystem on the microVM backend.
#[derive(Debug)]
pub struct SyncFs {
    workspace: PathBuf,
}

impl SyncFs {
    /// Flushes the filesystem that holds `workspace`.
    #[must_use]
    pub const fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

impl WorkspaceSync for SyncFs {
    fn sync(&self) -> Result<(), RuntimeError> {
        let flags = rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::CLOEXEC;
        let directory = rustix::fs::open(&self.workspace, flags, rustix::fs::Mode::empty())
            .map_err(|error| RuntimeError::Io(IoCause::of("workspace open", &error.into())))?;
        rustix::fs::syncfs(&directory)
            .map_err(|error| RuntimeError::Io(IoCause::of("workspace syncfs", &error.into())))
    }
}

/// The journal handle and its two bounded notification streams.
pub struct JournalSession {
    /// Acknowledged sink and replay operations over the single writer.
    pub journal: Arc<Journal>,
    /// Newly persisted frames for the supervisor's in-memory history.
    pub frames: mpsc::Receiver<ServerFrame>,
    /// Coalescing notification of the latest persisted frame; skipped frames remain in the WAL.
    pub changes: watch::Receiver<Option<Arc<ServerFrame>>>,
}
/// A sink that never acknowledges a frame before its entire WAL record and notifications exist.
pub struct Journal {
    commands: mpsc::Sender<Command>,
    task: Mutex<Option<JoinHandle<()>>>,
    sleeper: Arc<dyn Sleeper>,
    limits: RuntimeLimits,
    workspace_sync: Option<Arc<dyn WorkspaceSync>>,
    /// The cause of the last append the core's sink refused: the core keeps only a code, so the run's error takes the
    /// cause from here (GUARDRAILS §1.5).
    failure: Mutex<Option<RuntimeError>>,
    #[cfg(feature = "testing")]
    testing: crate::testing::TestingHooks,
}
impl Journal {
    /// Opens the WAL on an owned blocking worker, reporting torn-tail recovery without its contents.
    ///
    /// # Errors
    /// Rejects invalid limits, corrupt/locked storage and a missed acknowledgement deadline.
    pub async fn open(options: JournalOptions) -> Result<JournalSession, RuntimeError> {
        options.limits.validate()?;
        let (commands, receive) = mpsc::channel(options.limits.queue_capacity);
        let (frames, frame_receive) = mpsc::channel(options.limits.queue_capacity);
        let (changes, change_receive) = watch::channel(None);
        let (ready, opened) = oneshot::channel();
        let sleeper = options.sleeper.clone();
        let limits = options.limits;
        let workspace_sync = options.workspace_sync.clone();
        #[cfg(feature = "testing")]
        let testing = options.testing.clone();
        let task = tokio::task::spawn_blocking(move || {
            worker::run(options, receive, frames, changes, ready);
        });
        let journal = Arc::new(Self {
            commands,
            task: Mutex::new(Some(task)),
            sleeper,
            limits,
            workspace_sync,
            failure: Mutex::new(None),
            #[cfg(feature = "testing")]
            testing,
        });
        journal.receive(opened).await?;
        Ok(JournalSession {
            journal,
            frames: frame_receive,
            changes: change_receive,
        })
    }
    /// Truncates the acknowledged prefix and returns the first bounded page of the unacknowledged tail.
    /// Each page holds at most `queue_capacity` frames; continue with `records_after`.
    ///
    /// # Errors
    /// Rejects a reconnect watermark beyond this writer's maximum or a storage failure.
    pub async fn register(&self, watermark: Seq) -> Result<Records, RuntimeError> {
        let (reply, receive) = oneshot::channel();
        self.request(Command::Register { watermark, reply }, receive)
            .await
    }
    /// Returns the next retained WAL page after a sequence without mutating the log.
    /// Each page holds at most `queue_capacity` frames; an empty page marks the current end.
    ///
    /// # Errors
    /// Returns a worker or acknowledgement failure.
    pub async fn records_after(&self, after: Seq) -> Result<Records, RuntimeError> {
        let (reply, receive) = oneshot::channel();
        self.request(Command::Records { after, reply }, receive)
            .await
    }
    /// Appends `event` through the writer that owns the order, and answers its sequence.
    ///
    /// # Errors
    /// The failed step: the WAL write or its sync, the link's closed channel, a refused frame, or the worker gone.
    pub async fn append(&self, event: Event, context: EventContext) -> Result<Seq, RuntimeError> {
        let (reply, receive) = oneshot::channel();
        self.request(
            Command::Append {
                event: Box::new(event),
                context,
                reply,
            },
            receive,
        )
        .await
    }
    /// Takes the cause of the last append the core's sink refused, if any.
    pub fn take_failure(&self) -> Option<RuntimeError> {
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
    /// Flushes the existing log through the same writer that owns append ordering.
    ///
    /// # Errors
    /// Returns a storage or acknowledgement failure.
    pub async fn sync(&self) -> Result<(), RuntimeError> {
        let (reply, receive) = oneshot::channel();
        self.request(Command::Sync(reply), receive).await
    }
    /// Syncs and joins the owned writer. Call after the loop has joined all append users.
    ///
    /// # Errors
    /// Reports sync, acknowledgement or worker-join failure.
    pub async fn shutdown(&self) -> Result<(), RuntimeError> {
        #[cfg(feature = "testing")]
        self.testing.before_append.resume();
        let (reply, receive) = oneshot::channel();
        let result = self.request(Command::Stop(reply), receive).await;
        let task = self
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(mut task) = task {
            tokio::select! {biased;
                joined=&mut task=>{joined.map_err(|_|RuntimeError::Worker)?;},
                ()=self.sleeper.sleep(self.limits.io_timeout)=>{task.abort();return Err(RuntimeError::Timeout);}
            }
        }
        result
    }
    /// Flushes the workspace before a tool result is queued, within its own bound. A flush that finishes after the
    /// bound is harmless: no append was queued for it, so nothing is journaled late. Its blocking thread keeps running
    /// until `syncfs` returns (tokio's blocking pool bounds such threads). Parallel tool results each flush before their
    /// own append, so the order holds per result.
    async fn flush(&self, sync: Arc<dyn WorkspaceSync>) -> Result<(), RuntimeError> {
        let task = tokio::task::spawn_blocking(move || sync.sync());
        tokio::select! {biased;
            done = task => done.map_err(|_| RuntimeError::WorkspaceSync)?.map_err(|_| RuntimeError::WorkspaceSync),
            () = self.sleeper.sleep(self.limits.workspace_sync_timeout) => Err(RuntimeError::WorkspaceSync),
        }
    }
    async fn request<T>(
        &self,
        command: Command,
        receive: oneshot::Receiver<Result<T, RuntimeError>>,
    ) -> Result<T, RuntimeError> {
        tokio::select! {biased;
            sent=self.commands.send(command)=>sent.map_err(|_|RuntimeError::io("journal worker gone"))?,
            ()=self.sleeper.sleep(self.limits.io_timeout)=>return Err(RuntimeError::Timeout)
        }
        self.receive(receive).await
    }
    async fn receive<T>(
        &self,
        receive: oneshot::Receiver<Result<T, RuntimeError>>,
    ) -> Result<T, RuntimeError> {
        tokio::select! {biased;
            result=receive=>result.map_err(|_|RuntimeError::io("journal worker dropped its answer"))?,
            ()=self.sleeper.sleep(self.limits.io_timeout)=>Err(RuntimeError::Timeout)
        }
    }
}
#[async_trait]
impl EventSink for Journal {
    async fn emit(&self, event: Event, context: EventContext) -> Result<Seq, SinkError> {
        // The result's file changes are made durable before it is queued (ARCHITECTURE §4.5, `sync_before_result`).
        if matches!(event, Event::ToolResult { .. })
            && let Some(sync) = &self.workspace_sync
        {
            self.flush(sync.clone()).await.map_err(|error| {
                tracing::warn!(code = %error, "tool result not journaled");
                SinkError { code: error.code() }
            })?;
        }
        // The core keeps only a code; the cause (which step, the system's kind) is logged here (GUARDRAILS §1.5).
        self.append(event, context).await.map_err(|error| {
            tracing::warn!(error = %error, "an event was not journaled");
            let code = error.code();
            *self.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
            SinkError { code }
        })
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        #[cfg(feature = "testing")]
        self.testing.before_append.resume();
        if let Some(task) = self
            .task
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}
