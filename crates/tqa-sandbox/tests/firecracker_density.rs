//! SBX-F-009 and SBX-C-011: 16 microVMs at once on the reference host. All reach their workload, have unique names and
//! addresses, and stay within the memory budget: guest memory plus the VMM overhead each. The host is shared, so the
//! test refuses to start without headroom and aborts, destroying every VM, if the host runs low while it runs.

use std::{
    collections::BTreeSet,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use tokio_util::sync::CancellationToken;
use tqa_sandbox::{ByteSize, SandboxBackend, SandboxHandle};

#[path = "firecracker/support.rs"]
mod support;

use support::{Template, TestResult, host, run};

const VMS: u64 = 16;
const GUEST: u64 = 256 << 20;
/// Required free memory beyond the VMs themselves before the test starts (the host is shared).
const HEADROOM: u64 = 8 << 30;
/// Below this, the test destroys every VM and fails rather than squeeze the host.
const ABORT_BELOW: u64 = 4 << 30;

fn mem_available() -> Result<u64, Box<dyn std::error::Error>> {
    let meminfo = std::fs::read_to_string("/proc/meminfo")?;
    let kib: u64 = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))
        .and_then(|value| value.split_whitespace().next())
        .ok_or("no MemAvailable")?
        .parse()?;
    Ok(kib * 1024)
}

/// Samples the host every second; sets `low` and stops once `MemAvailable` falls below [`ABORT_BELOW`].
fn watchdog(low: Arc<AtomicBool>, stop: CancellationToken) -> tokio::task::JoinHandle<u64> {
    tokio::spawn(async move {
        let mut lowest = u64::MAX;
        while !stop.is_cancelled() {
            let available = mem_available().unwrap_or(0);
            lowest = lowest.min(available);
            if available < ABORT_BELOW {
                low.store(true, Ordering::SeqCst);
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        lowest
    })
}

/// What a VM that has not reached its workload is doing: its VM host container's state (running, or exited with
/// which code, and whether the engine killed it for memory) and the last lines it printed, so an exit is told apart
/// from a slow boot.
fn diagnose(handle: &SandboxHandle) -> String {
    let state = run(Command::new("docker").args([
        "inspect",
        "-f",
        "{{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} started={{.State.StartedAt}} \
         finished={{.State.FinishedAt}}",
        &handle.id,
    ]))
    .unwrap_or_else(|error| format!("inspect failed: {error}"));
    let tail = Command::new("docker")
        .args(["logs", "--tail", "12", &handle.id])
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout).into_owned()
                + &String::from_utf8_lossy(&output.stderr)
        })
        .unwrap_or_default();
    format!("{}: {state}\n{}", handle.id, tail.trim_end())
}

/// Waits until every VM's workload has started (the probe prints `holding`), or the host runs low. On failure, each
/// VM still waiting is described by [`diagnose`].
async fn all_running(handles: &[SandboxHandle], low: &AtomicBool) -> TestResult {
    let deadline = Instant::now() + Duration::from_mins(3);
    loop {
        let mut waiting = Vec::new();
        for handle in handles {
            if !support::console(handle)?.contains("tqa-probe: holding") {
                waiting.push(handle);
            }
        }
        if waiting.is_empty() {
            return Ok(());
        }
        if low.load(Ordering::SeqCst) || Instant::now() > deadline {
            let described: Vec<String> = waiting.iter().map(|handle| diagnose(handle)).collect();
            return Err(format!(
                "{} VMs not running; host low: {}\n{}",
                waiting.len(),
                low.load(Ordering::SeqCst),
                described.join("\n---\n")
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn address(id: &str) -> Result<String, Box<dyn std::error::Error>> {
    run(Command::new("docker").args([
        "inspect",
        "-f",
        "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
        id,
    ]))
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_f_009_sbx_c_011_sixteen_microvms_at_once_within_the_memory_budget() -> TestResult {
    // The VM host's allowance beside each guest's memory, as the backend is configured (`vmm_overhead`, 512 MiB by
    // default): connecting starts no VM, so the budget is known before any is created.
    let host = host(&Template::Probe).await?;
    let overhead = host.vmm_overhead;
    let budget = VMS * (GUEST + overhead);
    let before = mem_available()?;
    if before < budget + HEADROOM {
        return Err(format!(
            "not enough free memory to run: {before} < {} (ask for the RAM disk to be freed)",
            budget + HEADROOM
        )
        .into());
    }
    let (low, stop) = (Arc::new(AtomicBool::new(false)), CancellationToken::new());
    let lowest = watchdog(low.clone(), stop.clone());
    let started = Instant::now();
    let creates = (0..VMS).map(|_| {
        let mut spec = host.spec(&[("PROBE_CHECK", "hold")]);
        if let Ok(spec) = &mut spec {
            spec.memory = ByteSize::new(GUEST).unwrap_or(spec.memory);
        }
        let backend = &host.backend;
        async move {
            backend
                .create(spec?, CancellationToken::new())
                .await
                .map_err(Box::<dyn std::error::Error>::from)
        }
    });
    let results = futures::future::join_all(creates).await;
    let handles: Vec<SandboxHandle> = results
        .iter()
        .filter_map(|result| result.as_ref().ok().cloned())
        .collect();
    let outcome = match results.iter().find_map(|result| result.as_ref().err()) {
        Some(error) => Err(format!("a create failed: {error}").into()),
        None => check(&host.backend, &handles, &low, (budget, overhead), started).await,
    };
    for handle in &handles {
        host.backend.destroy(handle).await?;
    }
    stop.cancel();
    let lowest = lowest.await?;
    outcome?;
    assert!(
        !low.load(Ordering::SeqCst),
        "the host stayed above {ABORT_BELOW} bytes available (lowest {lowest})"
    );
    gone(&host.backend, &handles).await
}

async fn check(
    backend: &impl SandboxBackend,
    handles: &[SandboxHandle],
    low: &AtomicBool,
    (budget, overhead): (u64, u64),
    started: Instant,
) -> TestResult {
    all_running(handles, low).await?;
    let ready = started.elapsed();
    let names: BTreeSet<_> = handles.iter().map(|handle| handle.id.clone()).collect();
    let addresses: BTreeSet<_> = handles
        .iter()
        .map(|handle| address(&handle.id))
        .collect::<Result<_, _>>()?;
    assert_eq!(
        (names.len(), addresses.len()),
        (handles.len(), handles.len()),
        "unique names and addresses"
    );
    let mut used = 0;
    for handle in handles {
        let stats = backend.stats(handle).await?;
        assert!(
            stats.memory_bytes <= GUEST + overhead,
            "{}: {stats:?}",
            handle.id
        );
        used += stats.memory_bytes;
    }
    assert!(used <= budget, "{used} > {budget}");
    let record = serde_json::json!({"vms": handles.len(), "all_running_ms": ready.as_millis(), "memory_bytes": used, "budget_bytes": budget});
    std::fs::write(
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("sbx_f_009.json"),
        record.to_string(),
    )?;
    std::io::Write::write_fmt(&mut std::io::stdout(), format_args!("SBX-F-009 {record}\n"))?;
    Ok(())
}

/// Every VM host this test created is gone from the engine's listing (a removal may take a moment to show there).
/// The test network is shared with other runs of the KVM suites, so their VM hosts are reported, not counted.
async fn gone(backend: &impl SandboxBackend, created: &[SandboxHandle]) -> TestResult {
    let ours: BTreeSet<&str> = created.iter().map(|handle| handle.id.as_str()).collect();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let listed = backend.list().await?;
        let left: Vec<&SandboxHandle> = listed
            .iter()
            .filter(|handle| ours.contains(handle.id.as_str()))
            .collect();
        if left.is_empty() {
            let others: Vec<String> = listed
                .iter()
                .map(|handle| handle.conversation_id.to_string())
                .collect();
            if !others.is_empty() {
                std::io::Write::write_fmt(
                    &mut std::io::stdout(),
                    format_args!("other runs' VM hosts: {others:?}\n"),
                )?;
            }
            return Ok(());
        }
        if Instant::now() > deadline {
            let states: Vec<String> = left
                .iter()
                .map(|handle| {
                    run(Command::new("docker").args([
                        "inspect",
                        "-f",
                        "{{.State.Status}}",
                        &handle.id,
                    ]))
                    .unwrap_or_else(|error| error.to_string())
                })
                .collect();
            return Err(format!(
                "{} of this test's VM hosts left after destroy: {states:?}",
                left.len()
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
