//! SBX-F-028, SBX-F-030 to SBX-F-033: a busy container engine is retried, for the VM host's start and for a
//! new image, inside the start's create budget, each container create bounded like Docker's.
use std::{error::Error, fs, io, sync::Arc, time::Duration};

use tokio_util::sync::CancellationToken;

use super::tests::{Busy, FakeEngine, backend, host};
use super::{FirecrackerBackend, Retry};
use crate::firecracker::engine::Container;
use crate::{DEFAULT_CREATE_BUDGET, SandboxBackend, SandboxError};

impl FakeEngine {
    /// Whether the engine already holds `name`: a container with it, or a create still in flight under it. Docker
    /// answers a create under it 409, which the engine reports as the name held (SBX-F-033, SBX-F-035).
    pub(super) fn name_held(&self, name: &str) -> Result<bool, SandboxError> {
        let containers = self.containers.lock().map_err(|_| SandboxError::NotFound)?;
        let in_flight = self.in_flight.lock().map_err(|_| SandboxError::NotFound)?;
        Ok(containers.contains_key(name) || in_flight.contains_key(name))
    }

    /// Keeps a timed-out create in flight under `name`, as a busy engine does: it lands after two more look-ups.
    pub(super) fn hold(&self, name: &str, container: Container) -> Result<(), SandboxError> {
        self.in_flight
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .insert(name.into(), (2, container));
        Ok(())
    }

    /// A look-up of `name`: a create in flight under it comes one step closer to landing, and lands at the last.
    pub(super) fn land(&self, name: &str) -> Result<(), SandboxError> {
        let mut in_flight = self.in_flight.lock().map_err(|_| SandboxError::NotFound)?;
        let Some((left, _)) = in_flight.get_mut(name) else {
            return Ok(());
        };
        *left -= 1;
        if *left == 0
            && let Some((_, container)) = in_flight.remove(name)
        {
            self.containers
                .lock()
                .map_err(|_| SandboxError::NotFound)?
                .insert(name.into(), container);
        }
        Ok(())
    }

    /// Takes the engine's latency, or times out after `bound` when the latency is longer.
    pub(super) async fn latency(&self, bound: Duration) -> Result<(), SandboxError> {
        let latency = *self
            .create_latency
            .lock()
            .map_err(|_| SandboxError::NotFound)?;
        match latency {
            Some(latency) if latency > bound => {
                tokio::time::sleep(bound).await;
                Err(SandboxError::Timeout)
            }
            Some(latency) => {
                tokio::time::sleep(latency).await;
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Records a call's bound and, when it is to time out, spends it as the engine would.
    pub(super) async fn bounded(
        &self,
        call: &'static str,
        bound: Duration,
        times_out: bool,
    ) -> Result<(), SandboxError> {
        self.bounds
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .push((call, bound));
        if times_out && self.busy != Busy::Quick {
            tokio::time::sleep(bound).await;
        }
        Ok(())
    }
}

// TEST: SBX-F-028
#[tokio::test]
async fn sbx_f_028_a_busy_engine_is_retried_and_the_vm_starts() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    engine
        .create_timeouts
        .lock()
        .map_err(|_| "lock")?
        .extend([true, true]);
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    let backend = backend.with_retry(Retry::new(3, Duration::from_millis(1)));
    let handle = backend.create(spec, CancellationToken::new()).await?;
    assert_eq!(
        engine
            .create_attempts
            .load(std::sync::atomic::Ordering::SeqCst),
        3
    );
    assert_eq!(backend.list().await?, std::slice::from_ref(&handle));
    Ok(())
}

// TEST: SBX-F-028
#[tokio::test]
async fn sbx_f_028_an_engine_that_stays_busy_fails_after_its_attempts_and_leaves_nothing()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    engine
        .create_timeouts
        .lock()
        .map_err(|_| "lock")?
        .extend([true; 5]);
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    let backend = backend.with_retry(Retry::new(3, Duration::from_millis(1)));
    assert_eq!(
        backend.create(spec, CancellationToken::new()).await.err(),
        Some(SandboxError::Timeout)
    );
    assert_eq!(
        engine
            .create_attempts
            .load(std::sync::atomic::Ordering::SeqCst),
        3,
        "bounded attempts, never one per scripted timeout"
    );
    assert!(engine.containers.lock().map_err(|_| "lock")?.is_empty());
    let left: Vec<_> = fs::read_dir(root.path().join("sandboxes"))?.collect();
    assert!(left.is_empty(), "no sandbox directory or token is left");
    Ok(())
}

// TEST: SBX-F-030
#[tokio::test]
async fn sbx_f_030_a_busy_engine_is_retried_and_the_image_is_made_whole()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    engine
        .mkfs_timeouts
        .lock()
        .map_err(|_| "lock")?
        .push_back(true);
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    let backend = backend.with_retry(Retry::new(3, Duration::from_millis(1)));
    backend.create(spec, CancellationToken::new()).await?;
    // The one helper for both images timed out once and ran again (SBX-F-029).
    assert_eq!(
        engine
            .mkfs_attempts
            .load(std::sync::atomic::Ordering::SeqCst),
        2
    );
    for image in ["state/conversation_1.ext4", "workspaces/workspace_1.ext4"] {
        assert_eq!(
            fs::read(root.path().join(image))?,
            b"ext4",
            "{image} is the one a whole mkfs made, never the timed-out helper's part"
        );
        assert!(!root.path().join(format!("{image}.tmp")).exists());
    }
    Ok(())
}

// TEST: SBX-F-030
#[tokio::test]
async fn sbx_f_030_an_engine_that_stays_busy_fails_after_its_attempts_and_leaves_no_image()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    engine
        .mkfs_timeouts
        .lock()
        .map_err(|_| "lock")?
        .extend([true; 5]);
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    let backend = backend.with_retry(Retry::new(3, Duration::from_millis(1)));
    assert_eq!(
        backend.create(spec, CancellationToken::new()).await.err(),
        Some(SandboxError::Timeout)
    );
    assert_eq!(
        engine
            .mkfs_attempts
            .load(std::sync::atomic::Ordering::SeqCst),
        3
    );
    for image in [
        "state/conversation_1.ext4",
        "state/conversation_1.ext4.tmp",
        "workspaces/workspace_1.ext4",
        "workspaces/workspace_1.ext4.tmp",
    ] {
        assert!(
            !root.path().join(image).exists(),
            "no partial {image} is left for a later start to take as made"
        );
    }
    Ok(())
}

// TEST: SBX-F-030
#[tokio::test]
async fn sbx_f_030_each_retry_warning_names_what_it_retries() -> Result<(), Box<dyn Error>> {
    Capture::install();
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    engine
        .mkfs_timeouts
        .lock()
        .map_err(|_| "lock")?
        .push_back(true);
    engine
        .create_timeouts
        .lock()
        .map_err(|_| "lock")?
        .push_back(true);
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    let backend = backend.with_retry(Retry::new(3, Duration::from_millis(1)));
    backend.create(spec, CancellationToken::new()).await?;
    // The one image helper (SBX-F-029) and the VM host's start each timed out once and ran again.
    let attempts =
        |count: &std::sync::atomic::AtomicUsize| count.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        (
            attempts(&engine.mkfs_attempts),
            attempts(&engine.create_attempts)
        ),
        (2, 2)
    );
    let retries: Vec<String> = Capture::text()
        .lines()
        .filter(|line| line.contains("retrying"))
        .map(str::to_owned)
        .collect();
    // An operator reading a busy host's log can tell which container run the engine was slow to answer.
    assert_eq!(retries.len(), 2, "{retries:?}");
    assert!(retries[0].contains("making a block image"), "{retries:?}");
    assert!(retries[1].contains("starting a VM host"), "{retries:?}");
    Ok(())
}

/// The warnings logged on this test's thread. A scoped subscriber (`set_default`) misses events when tests run in
/// parallel: tracing's callsite interest and maximum level are process-wide, and a test without a subscriber can leave
/// them off. So one process-wide WARN subscriber writes each thread's events to that thread's own buffer.
struct Capture;

thread_local! {
    static WARNINGS: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Capture {
    /// Installs the process-wide subscriber once and starts this thread's buffer empty.
    fn install() {
        static INSTALLED: std::sync::Once = std::sync::Once::new();
        INSTALLED.call_once(|| {
            // Nothing else in this test binary sets a global subscriber; if one did, this test would fail, not pass.
            let _ = tracing::subscriber::set_global_default(
                tracing_subscriber::fmt()
                    .with_writer(|| Capture)
                    .with_ansi(false)
                    .with_max_level(tracing::Level::WARN)
                    .finish(),
            );
        });
        WARNINGS.with_borrow_mut(Vec::clear);
    }

    /// What this thread logged since `install`.
    fn text() -> String {
        WARNINGS.with_borrow(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }
}

impl io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        WARNINGS.with_borrow_mut(|buffer| buffer.extend_from_slice(bytes));
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// TEST: SBX-F-031
#[tokio::test(start_paused = true)]
async fn sbx_f_031_a_start_on_an_engine_that_stays_busy_fails_within_its_create_budget()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine {
        busy: Busy::SpendsBound,
        ..FakeEngine::default()
    });
    engine
        .create_timeouts
        .lock()
        .map_err(|_| "lock")?
        .extend([true; 5]);
    let (mut backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    backend.config.create_timeout = Duration::from_secs(20);
    backend.config.create_budget = Duration::from_secs(45);
    let started = tokio::time::Instant::now();
    assert_eq!(
        backend.create(spec, CancellationToken::new()).await.err(),
        Some(SandboxError::Timeout)
    );
    // A person waits at most the budget, though three attempts at the per-call bound would take 60 s and more.
    assert!(
        started.elapsed() <= Duration::from_secs(45),
        "{:?}",
        started.elapsed()
    );
    let creates: Vec<Duration> = engine
        .bounds
        .lock()
        .map_err(|_| "lock")?
        .iter()
        .filter(|(call, _)| *call == "create")
        .map(|(_, bound)| *bound)
        .collect();
    assert_eq!(
        creates.first(),
        Some(&Duration::from_secs(20)),
        "{creates:?}"
    );
    assert!(
        creates
            .last()
            .is_some_and(|last| *last < Duration::from_secs(20)),
        "the last attempt is cut to what is left: {creates:?}"
    );
    assert!(engine.containers.lock().map_err(|_| "lock")?.is_empty());
    Ok(())
}

/// An engine that needs `latency` for each container create and helper run, as a loaded host's did (44 s, candidate
/// 13h), behind a gateway whose ordinary calls are bounded at 10 s.
fn slow_engine(latency: Duration) -> Result<Arc<FakeEngine>, Box<dyn Error>> {
    let engine = Arc::new(FakeEngine::default());
    *engine.create_latency.lock().map_err(|_| "lock")? = Some(latency);
    Ok(engine)
}

// TEST: SBX-F-032
#[tokio::test(start_paused = true)]
async fn sbx_f_032_a_start_whose_creates_take_44_s_succeeds_within_the_create_bound()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine::default());
    let (mut backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    backend.config.operation_timeout = Duration::from_secs(10);
    // A new task's start runs two containers (the one image helper, then the VM host): 88 s on this engine, inside the
    // default budget of 120 s, which three (one helper per image) would not fit.
    assert_eq!(backend.config.create_budget, DEFAULT_CREATE_BUDGET);
    *engine.create_latency.lock().map_err(|_| "lock")? = Some(Duration::from_secs(44));
    // Each create gets the create bound (60 s), not the 10 s of an ordinary call: three 10 s attempts would all time
    // out on this engine however many there were.
    backend.create(spec, CancellationToken::new()).await?;
    assert!(
        engine
            .bounds
            .lock()
            .map_err(|_| "lock")?
            .iter()
            .all(|(_, bound)| *bound > Duration::from_secs(44)),
        "{:?}",
        engine.bounds.lock().map_err(|_| "lock")?
    );
    Ok(())
}

// TEST: SBX-F-032
#[tokio::test(start_paused = true)]
async fn sbx_f_032_start_up_probes_on_an_engine_whose_creates_take_44_s_succeed()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let (mut config, _) = host(root.path())?;
    config.operation_timeout = Duration::from_secs(10);
    FirecrackerBackend::with_engine(config, slow_engine(Duration::from_secs(44))?).await?;
    Ok(())
}

// TEST: SBX-F-033
#[tokio::test(start_paused = true)]
async fn sbx_f_033_creates_that_land_late_are_removed_by_their_own_name_and_the_start_ends_in_budget()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    // 13h's perf_start: a create timed out, the engine finished it anyway, and the retry hit the name (409).
    let engine = Arc::new(FakeEngine {
        busy: Busy::LandsLate,
        ..FakeEngine::default()
    });
    engine
        .create_timeouts
        .lock()
        .map_err(|_| "lock")?
        .extend([true, true]);
    let other = Container {
        name: "another-sandbox".into(),
        labels: std::collections::HashMap::new(),
        running: true,
        exit_code: None,
        address: None,
    };
    engine
        .containers
        .lock()
        .map_err(|_| "lock")?
        .insert(other.name.clone(), other.clone());
    let (mut backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    backend.config.create_timeout = Duration::from_secs(20);
    let started = tokio::time::Instant::now();
    let handle = backend.create(spec, CancellationToken::new()).await?;
    assert!(started.elapsed() <= backend.config.create_budget);
    // Both late creates were removed before the next attempt, which then got the name.
    assert_eq!(
        engine
            .create_attempts
            .load(std::sync::atomic::Ordering::SeqCst),
        3
    );
    assert_eq!(backend.list().await?, std::slice::from_ref(&handle));
    let containers = engine.containers.lock().map_err(|_| "lock")?.clone();
    let own: Vec<_> = containers
        .keys()
        .filter(|name| **name != other.name)
        .collect();
    let removed = engine.removed.lock().map_err(|_| "lock")?.clone();
    assert_eq!(own.len(), 1, "{containers:?}");
    assert!(
        removed.len() >= 2 && removed.iter().all(|name| name == own[0]),
        "only this sandbox's own name is removed: {removed:?}"
    );
    assert_eq!(containers.get(&other.name), Some(&other));
    Ok(())
}

// TEST: SBX-F-033
#[tokio::test(start_paused = true)]
async fn sbx_f_033_a_name_still_taken_after_the_last_attempt_fails_with_a_timeout_in_budget()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine {
        busy: Busy::LandsLate,
        ..FakeEngine::default()
    });
    engine
        .create_timeouts
        .lock()
        .map_err(|_| "lock")?
        .extend([true; 5]);
    let (backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    let started = tokio::time::Instant::now();
    assert_eq!(
        backend.create(spec, CancellationToken::new()).await.err(),
        Some(SandboxError::Timeout)
    );
    assert!(started.elapsed() <= backend.config.create_budget);
    Ok(())
}

// TEST: SBX-F-035
/// The microVM's half of SBX-U-015: attempt 1's create outlasts its bound and stays in flight, holding the name; the
/// retry's create finds it held. It is waited for within the budget, removed once it lands (this sandbox's own name),
/// and the VM host is created again, without spending an attempt.
#[tokio::test(start_paused = true)]
async fn sbx_f_035_a_create_still_in_flight_is_waited_for_then_removed_without_using_up_an_attempt()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let engine = Arc::new(FakeEngine {
        busy: Busy::InFlight,
        ..FakeEngine::default()
    });
    engine
        .create_timeouts
        .lock()
        .map_err(|_| "lock")?
        .push_back(true);
    let (mut backend, spec) = backend(root.path(), Arc::clone(&engine)).await?;
    backend.config.create_timeout = Duration::from_secs(20);
    // Two attempts: the held name must not spend the second.
    let backend = backend.with_retry(Retry::new(2, Duration::from_millis(1)));
    let started = tokio::time::Instant::now();
    let handle = backend.create(spec, CancellationToken::new()).await?;
    assert!(started.elapsed() <= backend.config.create_budget);
    assert_eq!(backend.list().await?, std::slice::from_ref(&handle));
    assert_eq!(
        engine
            .create_attempts
            .load(std::sync::atomic::Ordering::SeqCst),
        3,
        "timed out, found the name held, created after the late one was removed"
    );
    let removed = engine.removed.lock().map_err(|_| "lock")?.clone();
    let own = engine
        .containers
        .lock()
        .map_err(|_| "lock")?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        !removed.is_empty() && removed.iter().all(|name| own.contains(name)),
        "only this sandbox's own name is removed: {removed:?} {own:?}"
    );
    assert!(engine.in_flight.lock().map_err(|_| "lock")?.is_empty());
    Ok(())
}
