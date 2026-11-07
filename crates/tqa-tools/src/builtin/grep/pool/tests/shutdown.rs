//! TOOL-U-005: stop cannot lose its notification as a full-window worker begins waiting.
use super::*;

#[test]
fn tool_u_005_parallel_stop_wakes_a_worker_entering_the_window_wait() -> TestResult {
    let directory = tempfile::tempdir()?;
    let cx = context(&directory)?;
    let args = crate::builtin::arguments(json!({"pattern":"hit"}))?;
    let matcher = super::super::super::matcher(&args)?;
    let page = super::super::super::page(&args, &cx)?;
    let (mut probe, events) = Probe::new(true);
    probe.hold_window = true;
    let query = Query {
        args: &args,
        matcher: &matcher,
        cx: &cx,
        cancel: &cx.cancel,
        started: cx.clock.now(),
        probe: Some(&probe),
    };
    let paths = vec![PathBuf::new(); MAX_PENDING_FILES + 1];
    let pool = Pool {
        query: &query,
        paths: &paths,
        capture: Capture::new(&page),
        cancel: cx.cancel.child_token(),
        state: Mutex::new(State {
            next: MAX_PENDING_FILES,
            merged: 0,
            allowance: cx.limits.search_max_bytes,
            running: 1,
            failed: false,
            results: BTreeMap::new(),
        }),
        changed: Condvar::new(),
        large_files: LargeFiles::default(),
    };
    assert_stops_at_wait(&pool, &probe, &events)
}

fn assert_stops_at_wait(pool: &Pool<'_>, probe: &Probe, events: &Receiver<Event>) -> TestResult {
    std::thread::scope(|scope| {
        let (claimed, completion) = mpsc::channel();
        let worker = scope.spawn(move || {
            let result = pool.claim();
            let _ = claimed.send(());
            result
        });
        let window = events.recv_timeout(Duration::from_secs(5));
        let (stopped, stopping) = mpsc::channel();
        let owner = scope.spawn(move || {
            drop(Stop(pool));
            let _ = stopped.send(());
        });
        // Observe real threads: the broken stop finishes while the worker still holds the lock.
        // A synchronized stop waits for the gate; bound that observation, then release it.
        let _ = stopping.recv_timeout(Duration::from_millis(200));
        probe.release();
        let returned = completion.recv_timeout(Duration::from_secs(5));
        // Rescue a missed wake-up before asserting, so a regression never leaks a test worker.
        {
            let _state = pool.lock()?;
            pool.changed.notify_all();
        }
        let claimed = worker.join().map_err(|_| "claim worker panicked")??;
        owner.join().map_err(|_| "stop worker panicked")?;
        assert!(matches!(window?, Event::WindowFull));
        assert!(
            returned.is_ok(),
            "stop notification was lost before the worker waited"
        );
        assert_eq!(claimed, None);
        Ok(())
    })
}
