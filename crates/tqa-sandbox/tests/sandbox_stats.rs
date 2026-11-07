//! SBX-C-009: a busy Docker sandbox reports plausible CPU, memory, process and uptime figures. The reference is the
//! sandbox's own cgroup, read from inside it, plus the fixture's limits: half a CPU, 64 MiB and 64 processes.

use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;
use tqa_sandbox::SandboxBackend;

#[path = "support/docker.rs"]
mod support;

use support::{Fixture, TestResult, run};

const LIMIT: u64 = 64 * 1024 * 1024;
const HELD: u64 = 16_000_000;

/// One number from the sandbox's own cgroup, read as the sandbox user.
async fn cgroup(
    fixture: &Fixture,
    id: &str,
    script: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let (exit, output) = fixture.exec(id, &["sh", "-c", script]).await?;
    assert_eq!(exit, 0, "{script}: {output}");
    Ok(output.trim().parse()?)
}

const CPU_MICROS: &str = "sed -n 's/^usage_usec //p' /sys/fs/cgroup/cpu.stat";
const MEMORY: &str = "cat /sys/fs/cgroup/memory.current";

#[tokio::test]
#[ignore = "needs Docker; run with --run-ignored all and TQA_DOCKER_TESTS=1"]
async fn sbx_c_009_a_busy_sandbox_reports_plausible_cpu_and_memory() -> TestResult {
    run(async |fixture| {
        let created = Instant::now();
        let handle = fixture
            .backend
            .create(fixture.spec("stats", false)?, CancellationToken::new())
            .await?;
        let (idle, cgroup_before) = (
            fixture.backend.stats(&handle).await?,
            cgroup(fixture, &handle.id, CPU_MICROS).await?,
        );
        // Copying 24 GB of zeros is about a second of CPU, bounded by work rather than a clock, with no memory growth.
        // (A toybox shell loop leaks memory, and its builtins cannot be killed by name.)
        let spin = "head -c 24000000000 /dev/zero > /dev/null";
        assert_eq!(fixture.exec(&handle.id, &["sh", "-c", spin]).await?.0, 0);
        let cgroup_after = cgroup(fixture, &handle.id, CPU_MICROS).await?;
        let busy = fixture.backend.stats(&handle).await?;
        let (reported, measured) = (
            busy.cpu_millis - idle.cpu_millis,
            (cgroup_after - cgroup_before) / 1000,
        );
        assert!(reported >= 300, "{reported} ms reported for a busy loop");
        assert!(
            reported.abs_diff(measured) <= 50 + measured / 10,
            "{reported} ms reported, {measured} ms measured"
        );
        // Tmpfs pages are charged to the sandbox's memory: 16 MB held in /dev/shm, plus one background process.
        let holder =
            format!("head -c {HELD} /dev/zero > /dev/shm/held && (sleep 60 >/dev/null 2>&1 &)");
        assert_eq!(fixture.exec(&handle.id, &["sh", "-c", &holder]).await?.0, 0);
        let held = fixture.backend.stats(&handle).await?;
        let current = cgroup(fixture, &handle.id, MEMORY).await?;
        // Absolute rather than a rise over the busy sample, which still holds the finished exec's transient memory.
        assert!(
            held.memory_bytes >= HELD,
            "the 16 MB held in /dev/shm: {held:?}"
        );
        // Reading memory.current runs another exec, which only adds to it; 1 MiB allows for reclaim in between.
        assert!(
            held.memory_bytes <= current + (1 << 20),
            "{held:?} against memory.current {current}"
        );
        assert!(held.memory_bytes <= LIMIT, "{held:?}");
        assert!(
            (2..=64).contains(&held.pids),
            "the entrypoint and the background sleep: {held:?}"
        );
        assert!(
            held.uptime <= created.elapsed() + Duration::from_secs(2),
            "{held:?}"
        );
        assert!(held.cpu_millis >= busy.cpu_millis, "CPU time is cumulative");
        Ok(())
    })
    .await
}
