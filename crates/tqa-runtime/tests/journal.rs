//! RT-I-005: acknowledgement follows a complete WAL record, with replay and explicit flush boundaries.
use std::sync::Arc;
use tempfile::TempDir;
use tqa_core::{EventContext, EventSink};
use tqa_runtime::{Journal, JournalOptions, RuntimeError, RuntimeLimits, WallClock};
use tqa_types::{Event, Seq};

struct FixedWall;
impl WallClock for FixedWall {
    fn timestamp(&self) -> Result<String, RuntimeError> {
        Ok("2000-01-01T00:00:00Z".into())
    }
}
fn options(directory: &TempDir) -> JournalOptions {
    JournalOptions {
        directory: directory.path().into(),
        wall_clock: Arc::new(FixedWall),
        sleeper: Arc::new(tqa_llm::TokioSleeper),
        limits: RuntimeLimits::RECOMMENDED,
        workspace_sync: None,
        testing: tqa_runtime::testing::TestingHooks::default(),
    }
}
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn rt_i_005_acknowledged_frame_is_durable_before_it_is_visible() -> TestResult {
    let directory = TempDir::new()?;
    let mut session = Journal::open(options(&directory)).await?;
    assert!(session.journal.register(Seq::ZERO).await?.is_empty());
    let seq = session
        .journal
        .emit(
            Event::MessageUser {
                lead_brief: None,
                team_wake: None,
                text: "question".into(),
                attachments: vec![],
            },
            EventContext::default(),
        )
        .await?;
    let frame = session.frames.recv().await.ok_or("missing frame")?;
    assert_eq!(seq, Seq::from(1));
    let bytes = std::fs::read(directory.path().join("events.jsonl"))?;
    assert_eq!(bytes.last(), Some(&b'\n'));
    let record: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(record["seq"], 1);
    assert_eq!(record["frame"], serde_json::to_value(&frame)?);
    session.journal.shutdown().await?;
    let mut resumed = Journal::open(options(&directory)).await?;
    assert_eq!(
        resumed.journal.register(Seq::ZERO).await?,
        vec![(seq, frame)]
    );
    assert!(resumed.journal.register(seq).await?.is_empty());
    assert_eq!(
        resumed
            .journal
            .emit(
                Event::MessageUser {
                    lead_brief: None,
                    team_wake: None,
                    text: "next".into(),
                    attachments: vec![]
                },
                EventContext::default()
            )
            .await?,
        Seq::from(2)
    );
    resumed.frames.recv().await.ok_or("missing next frame")?;
    assert!(resumed.journal.register(Seq::from(3)).await.is_err());
    resumed.journal.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn rt_i_005_oversize_frames_fail_without_writing_or_advancing_sequence() -> TestResult {
    let directory = TempDir::new()?;
    let session = Journal::open(options(&directory)).await?;
    session.journal.register(Seq::ZERO).await?;
    let result = session
        .journal
        .emit(
            Event::TextDelta {
                text: "x".repeat(tqa_types::MAX_FRAME_BYTES),
            },
            EventContext::default(),
        )
        .await;
    assert!(result.is_err());
    assert!(std::fs::read(directory.path().join("events.jsonl"))?.is_empty());
    assert!(session.journal.records_after(Seq::ZERO).await?.is_empty());
    session.journal.shutdown().await?;
    Ok(())
}

/// Records, at each flush, whether the journal already held a tool result; optionally fails.
struct Recorder {
    journal: std::path::PathBuf,
    seen: std::sync::Mutex<Vec<bool>>,
    fail: bool,
    delay: std::time::Duration,
    /// When set, the flush waits until the test opens the gate, and says when it has finished: a slow flush that
    /// depends on no clock or disk speed.
    gate: Option<Arc<Gate>>,
}
/// A flush held until the test opens it, then reported finished.
#[derive(Default)]
struct Gate {
    open: std::sync::Mutex<bool>,
    opened: std::sync::Condvar,
    finished: std::sync::Mutex<bool>,
    done: std::sync::Condvar,
}
impl Gate {
    fn open(&self) {
        *self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.opened.notify_all();
    }
    fn wait_open(&self) {
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !*open {
            open = self
                .opened
                .wait(open)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
    fn finish(&self) {
        *self
            .finished
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.done.notify_all();
    }
    /// Waits for the flush to finish, bounded generously so a stuck flush fails the test instead of hanging it.
    fn wait_finished(&self) -> bool {
        let finished = self
            .finished
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (finished, _) = self
            .done
            .wait_timeout_while(finished, std::time::Duration::from_secs(30), |finished| {
                !*finished
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *finished
    }
}
impl tqa_runtime::WorkspaceSync for Recorder {
    fn sync(&self) -> Result<(), RuntimeError> {
        std::thread::sleep(self.delay);
        if let Some(gate) = &self.gate {
            gate.wait_open();
        }
        let text = std::fs::read_to_string(&self.journal).unwrap_or_default();
        self.seen
            .lock()
            .map_err(|_| RuntimeError::io("test lock"))?
            .push(text.contains("\"tool.result\""));
        if let Some(gate) = &self.gate {
            gate.finish();
        }
        if self.fail {
            Err(RuntimeError::io("test sync"))
        } else {
            Ok(())
        }
    }
}
fn tool_result() -> Result<Event, Box<dyn std::error::Error>> {
    Ok(Event::ToolResult {
        call_id: "call-1".parse()?,
        status: tqa_types::ToolStatus::Ok,
        content: "done".into(),
        target: None,
        evidence: None,
    })
}
fn recorder(directory: &TempDir, fail: bool) -> Arc<Recorder> {
    Arc::new(Recorder {
        journal: directory.path().join("events.jsonl"),
        seen: std::sync::Mutex::new(Vec::new()),
        fail,
        delay: std::time::Duration::ZERO,
        gate: None,
    })
}
#[tokio::test]
async fn rt_i_024_the_workspace_is_flushed_before_a_tool_result_is_journaled() -> TestResult {
    let directory = TempDir::new()?;
    let flushes = recorder(&directory, false);
    let mut configured = options(&directory);
    configured.workspace_sync = Some(flushes.clone());
    let session = Journal::open(configured).await?;
    session.journal.register(Seq::ZERO).await?;
    let message = Event::MessageUser {
        lead_brief: None,
        team_wake: None,
        text: "go".into(),
        attachments: vec![],
    };
    session
        .journal
        .emit(message, EventContext::default())
        .await?;
    session
        .journal
        .emit(tool_result()?, EventContext::default())
        .await?;
    // Exactly one flush, for the result, taken while the result was not yet in the journal.
    assert_eq!(*flushes.seen.lock().map_err(|_| "lock")?, vec![false]);
    let journal = std::fs::read_to_string(directory.path().join("events.jsonl"))?;
    assert!(
        journal.contains("\"tool.result\""),
        "the result was journaled after the flush"
    );
    session.journal.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn rt_i_024_a_failed_flush_journals_no_result() -> TestResult {
    let directory = TempDir::new()?;
    let mut configured = options(&directory);
    configured.workspace_sync = Some(recorder(&directory, true));
    let session = Journal::open(configured).await?;
    session.journal.register(Seq::ZERO).await?;
    assert!(
        session
            .journal
            .emit(tool_result()?, EventContext::default())
            .await
            .is_err()
    );
    let journal =
        std::fs::read_to_string(directory.path().join("events.jsonl")).unwrap_or_default();
    assert!(
        !journal.contains("\"tool.result\""),
        "a result whose files may not be durable is never journaled"
    );
    Ok(())
}
#[tokio::test]
async fn rt_i_024_without_the_backend_property_nothing_is_flushed() -> TestResult {
    let directory = TempDir::new()?;
    let session = Journal::open(options(&directory)).await?;
    session.journal.register(Seq::ZERO).await?;
    session
        .journal
        .emit(tool_result()?, EventContext::default())
        .await?;
    assert!(
        options(&directory).workspace_sync.is_none(),
        "Docker and tests default to no flush"
    );
    session.journal.shutdown().await?;
    Ok(())
}
#[test]
fn rt_i_024_syncfs_flushes_the_workspace_filesystem_and_fails_closed() -> TestResult {
    use tqa_runtime::WorkspaceSync;
    // On tmpfs: the real call, without flushing the host's disk (syncfs covers the whole filesystem).
    let directory = tempfile::tempdir_in("/dev/shm")?;
    std::fs::write(directory.path().join("pending"), "written")?;
    tqa_runtime::SyncFs::new(directory.path().into()).sync()?;
    let missing = tqa_runtime::SyncFs::new(directory.path().join("absent"));
    assert!(
        matches!(missing.sync(), Err(RuntimeError::Io(cause)) if cause.step == "workspace open" && cause.kind == Some(std::io::ErrorKind::NotFound)),
        "a missing workspace names the open and its kind"
    );
    Ok(())
}
#[tokio::test]
async fn rt_i_024_a_flush_beyond_its_bound_fails_the_result_and_never_journals_it_late()
-> TestResult {
    let directory = TempDir::new()?;
    // The flush is held until the test opens its gate, so it outlives its bound whatever the disk or the load.
    let gate = Arc::new(Gate::default());
    let slow = Arc::new(Recorder {
        journal: directory.path().join("events.jsonl"),
        seen: std::sync::Mutex::new(Vec::new()),
        fail: false,
        delay: std::time::Duration::ZERO,
        gate: Some(gate.clone()),
    });
    let mut configured = options(&directory);
    configured.workspace_sync = Some(slow.clone());
    // The flush's own bound, far below the append deadline (`io_timeout`, 5 s).
    configured.limits = RuntimeLimits {
        workspace_sync_timeout: std::time::Duration::from_millis(50),
        ..RuntimeLimits::RECOMMENDED
    };
    let session = Journal::open(configured).await?;
    session.journal.register(Seq::ZERO).await?;
    let started = std::time::Instant::now();
    let failed = session
        .journal
        .emit(tool_result()?, EventContext::default())
        .await;
    assert_eq!(
        failed.err().map(|error| error.code),
        Some(tqa_types::ErrorCode::Io)
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "the flush's bound (50 ms), not the append's (5 s)"
    );
    // Let the held flush finish, wait until it has, then prove nothing was journaled for it.
    gate.open();
    let finished = tokio::task::spawn_blocking({
        let gate = gate.clone();
        move || gate.wait_finished()
    })
    .await?;
    assert!(finished, "the held flush finished once released");
    assert_eq!(
        slow.seen.lock().map_err(|_| "lock")?.len(),
        1,
        "the flush did finish"
    );
    let message = Event::MessageUser {
        lead_brief: None,
        team_wake: None,
        text: "next".into(),
        attachments: vec![],
    };
    let next = session
        .journal
        .emit(message, EventContext::default())
        .await?;
    assert_eq!(next, Seq::from(1), "the failed result consumed no sequence");
    let journal = std::fs::read_to_string(directory.path().join("events.jsonl"))?;
    assert!(!journal.contains("\"tool.result\""), "no late append");
    session.journal.shutdown().await?;
    Ok(())
}
