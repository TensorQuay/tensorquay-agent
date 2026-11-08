//! The sandbox contract on the Firecracker backend, from inside real microVMs and from the host (SBX-C on firecracker,
//! SBX-F-002, SBX-F-003, SBX-F-004, PERF-011).
//!
//! A probe template (tests/firecracker/probe) replaces the runtime as the image entrypoint; the guest init runs it as the
//! sandbox user, and its verdict returns through the exit record. Dial-in and egress are observed on a gateway stub on
//! the internal test network. Requirements and how to run: tests/firecracker/support.rs.

use std::{io::Write, path::Path, process::Command, time::Duration};

use tokio_util::sync::CancellationToken;
use tqa_sandbox::SandboxBackend;

#[path = "../../tqa-core/tests/performance/host.rs"]
mod perf_host;
#[path = "firecracker/perf_record.rs"]
mod perf_record;
#[path = "firecracker/support.rs"]
mod support;

use support::{Template, TestResult, engine_uid, host, run};

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_005_006_007_016_posture_holds_inside_a_firecracker_guest() -> TestResult {
    let host = host(&Template::Probe).await?;
    let (code, report) = host.probe("posture").await?;
    assert_eq!(
        code, 0,
        "the posture probe failed inside the guest: {report}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_f_003_the_vmm_runs_unprivileged_and_confined() -> TestResult {
    let host = host(&Template::Probe).await?;
    let spec = host.spec(&[("PROBE_CHECK", "hold")])?;
    let memory = spec.memory.bytes();
    let handle = host.backend.create(spec, CancellationToken::new()).await?;
    // Observing a real process: the launcher execs Firecracker within moments of the container starting.
    let mut pid = String::new();
    for _ in 0..60 {
        let top = run(Command::new("docker").args(["top", &handle.id, "-eo", "pid,args"]))?;
        if let Some(line) = top
            .lines()
            .find(|line| line.contains("/usr/local/bin/firecracker"))
        {
            pid = line
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned();
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let status = std::fs::read_to_string(format!("/proc/{pid}/status"));
    let inspect = run(Command::new("docker").args([
        "inspect",
        "-f",
        "{{.HostConfig.ReadonlyRootfs}} {{.HostConfig.Memory}} {{.HostConfig.Privileged}}",
        &handle.id,
    ]));
    host.backend.destroy(&handle).await?;
    let status = status.map_err(|_| "the Firecracker process was not observed")?;
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}:")))
            .map(str::trim)
            .unwrap_or_default()
            .to_owned()
    };
    let vmm = engine_uid(10_000)?.to_string();
    assert_eq!(
        field("Uid").split_whitespace().collect::<Vec<_>>(),
        [vmm.as_str(); 4],
        "the VMM uid, never the service account"
    );
    assert_ne!(vmm, run(Command::new("id").arg("-u"))?);
    for set in ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"] {
        assert_eq!(field(set), "0000000000000000", "{set}");
    }
    assert_eq!(
        (field("NoNewPrivs"), field("Seccomp")),
        ("1".to_owned(), "2".to_owned())
    );
    // The guest's memory plus the configured VM host overhead (512 MiB by default since the G-13-full fix).
    assert_eq!(
        inspect?,
        format!("true {} false", memory + host.vmm_overhead)
    );
    Ok(())
}

fn unix_now() -> Result<u64, Box<dyn std::error::Error>> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_004_sbx_f_004_a_guest_reaches_only_the_gateway_link() -> TestResult {
    let host = host(&Template::Probe).await?;
    let since = unix_now()?.saturating_sub(1).to_string();
    let (code, report) = host.probe("egress").await?;
    assert_eq!(
        code, 0,
        "the egress probe failed inside the guest: {report}"
    );
    // The stub is a second witness: it saw the link port, and never the blocked port on the same address.
    let lines = support::stub_lines(&since)?;
    assert!(
        lines
            .iter()
            .any(|line| line.contains(&format!("port={} ", support::LINK_PORT))),
        "{lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains(&format!("port={} ", support::CLOSED_PORT))),
        "{lines:?}"
    );
    Ok(())
}

/// Boots the real runtime and returns how long create took until it dialled the gateway stub.
async fn boot_to_dial(
    host: &support::Host,
    spec: tqa_sandbox::SandboxSpec,
) -> Result<Duration, Box<dyn std::error::Error>> {
    let since = unix_now()?.saturating_sub(1).to_string();
    let started = std::time::Instant::now();
    let handle = host.backend.create(spec, CancellationToken::new()).await?;
    let peer = host.backend.address(&handle).await?;
    let ready = support::Dialled {
        since: since.clone(),
        peer: std::sync::Mutex::new(Some(peer.to_string())),
    };
    let waited = host
        .backend
        .wait_ready(
            &handle,
            &ready,
            Duration::from_mins(1),
            CancellationToken::new(),
        )
        .await;
    let elapsed = started.elapsed();
    let lines = support::stub_lines(&since)?;
    let console = support::full_console(&handle)?;
    host.backend.destroy(&handle).await?;
    waited.map_err(|error| format!("{error}; guest console tail:\n{console}"))?;
    let dial = lines
        .iter()
        .find(|line| {
            line.contains(&format!("peer={peer} "))
                && line.contains(&format!("port={} ", support::LINK_PORT))
        })
        .ok_or("no dial from the VM host")?;
    assert!(
        dial.contains("auth=yes"),
        "the runtime dialled without its credential: {dial}"
    );
    Ok(elapsed)
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_f_002_the_runtime_boots_and_dials_the_gateway() -> TestResult {
    let host = host(&Template::Runtime).await?;
    let elapsed = boot_to_dial(&host, host.spec(&[])?).await?;
    writeln!(
        std::io::stdout(),
        "SBX-F-002 create to first dial: {} ms",
        elapsed.as_millis()
    )?;
    Ok(())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn perf_011_firecracker_cold_boot_to_dial() -> TestResult {
    // PRD M3: at most 2 s p50 from create to the runtime dialling in, with the workspace and state images existing
    // (a returning conversation). The first boot creates them and is not sampled.
    let runs: usize = std::env::var("TQA_FC_PERF_RUNS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5);
    if runs < 1 {
        return Err("TQA_FC_PERF_RUNS must be at least 1".into());
    }
    let host = host(&Template::Runtime).await?;
    let spec = host.spec(&[])?;
    // The environment before the runs (TEST-PLAN §15.1), in the stack harnesses' forms (perf_common.environment).
    let mut environment = perf_host::environment()?;
    environment["utc"] =
        perf_record::extended_utc(environment["utc"].as_str().ok_or("missing UTC")?)
            .ok_or("bad UTC stamp")?
            .into();
    environment["images"] = perf_011_images()?;
    let docker = |arguments: &[&str]| run(Command::new("docker").args(arguments));
    environment["docker"] = docker(&["version", "--format", "{{.Server.Version}}"])?.into();
    environment["compose"] = docker(&["compose", "version", "--short"])?.into();
    environment["rootless"] = docker(&["info", "--format", "{{json .SecurityOptions}}"])?
        .contains("rootless")
        .into();
    environment["load_before"] = perf_load()?;
    environment["io_pressure_before"] = perf_record::io_pressure().into();
    boot_to_dial(&host, spec.clone()).await?;
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        samples.push(boot_to_dial(&host, spec.clone()).await?.as_millis());
    }
    environment["load_after"] = perf_load()?;
    environment["io_pressure_after"] = perf_record::io_pressure().into();
    let utc = environment["utc"]
        .as_str()
        .ok_or("missing UTC stamp")?
        .to_owned();
    let result = perf_record::record(
        samples,
        &environment,
        &serde_json::json!({"runs": runs, "percentile": "nearest-rank",
            "measure": "create to the runtime dialling in, a returning conversation's existing workspace and state \
                images; the first boot creates them and is not sampled"}),
    )
    .ok_or("no samples")?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .ok_or("missing workspace root")?;
    let path = perf_record::path(root, &utc).ok_or("bad UTC stamp")?;
    std::fs::create_dir_all(path.parent().ok_or("missing results directory")?)?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?
        .write_all(&serde_json::to_vec_pretty(&result)?)?;
    writeln!(
        std::io::stdout(),
        "PERF-011 {} result={}",
        result["summary"],
        path.display()
    )?;
    assert_eq!(
        result["verdict"], "passed",
        "PERF-011 p50 exceeds 2000 ms: {}",
        result["summary"]
    );
    Ok(())
}

/// The host's load averages, as `os.getloadavg()` gives them on the Python side.
fn perf_load() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    perf_record::load_average(&std::fs::read_to_string("/proc/loadavg")?)
        .ok_or_else(|| "bad /proc/loadavg".into())
}

/// The images a PERF-011 run boots, as their IDs and the kernel's manifest digest.
fn perf_011_images() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // The same defaults as the host (support.rs): the runtime template's image is optional.
    let id =
        |variable: &str, default: Option<&str>| -> Result<String, Box<dyn std::error::Error>> {
            let image = match (std::env::var(variable), default) {
                (Ok(image), _) => image,
                (Err(_), Some(default)) => default.to_owned(),
                (Err(error), None) => return Err(error.into()),
            };
            run(Command::new("docker").args(["image", "inspect", "--format", "{{.Id}}", &image]))
        };
    let kernel = Path::new(&std::env::var("TQA_FC_KERNEL_DIR")?).join("manifest.json");
    let digest = run(Command::new("sha256sum").arg(&kernel))?;
    Ok(serde_json::json!({
        "vmm": id("TQA_VMM_IMAGE", None)?,
        "runtime": id("TQA_FC_BASE_IMAGE", Some("tqa-runtime:base"))?,
        "kernel_manifest_sha256": digest.split_whitespace().next().unwrap_or_default(),
    }))
}

/// CPU seconds of the Firecracker process (all threads, guest vCPU time included), from the host's `/proc`.
fn vmm_cpu_millis(id: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let top = run(Command::new("docker").args(["top", id, "-eo", "pid,args"]))?;
    let pid = top
        .lines()
        .find(|line| line.contains("/usr/local/bin/firecracker"))
        .and_then(|line| line.split_whitespace().next())
        .ok_or("the Firecracker process was not observed")?
        .to_owned();
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // Fields after the parenthesised command name; utime and stime are the 12th and 13th of them.
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .ok_or("malformed stat")?
        .1
        .split_whitespace()
        .collect();
    let ticks: u64 = fields[11].parse::<u64>()? + fields[12].parse::<u64>()?;
    let per_second: u64 = run(Command::new("getconf").arg("CLK_TCK"))?
        .trim()
        .parse()?;
    Ok(ticks * 1000 / per_second)
}

/// Waits for the busy probe's marker: its CPU work is done and 128 MiB are held.
async fn until_held(handle: &tqa_sandbox::SandboxHandle) -> TestResult {
    for _ in 0..240 {
        if support::console(handle)?.contains("tqa-probe: busy-held") {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(format!(
        "the busy probe never held its memory: {}",
        support::console(handle)?
    )
    .into())
}

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_c_009_a_busy_microvm_reports_plausible_cpu_and_memory() -> TestResult {
    // On this backend the figures are the VM host container's: CPU includes the guest's vCPU time, memory is the guest
    // RAM the VMM has touched plus its own, and pids counts VMM tasks rather than guest processes.
    let host = host(&Template::Probe).await?;
    let spec = host.spec(&[("PROBE_CHECK", "busy")])?;
    let limit = spec.memory.bytes() + host.vmm_overhead;
    let created = std::time::Instant::now();
    let handle = host.backend.create(spec, CancellationToken::new()).await?;
    let sampled = async {
        until_held(&handle).await?;
        let stats = host.backend.stats(&handle).await?;
        Ok::<_, Box<dyn std::error::Error>>((stats, vmm_cpu_millis(&handle.id)?))
    }
    .await;
    host.backend.destroy(&handle).await?;
    let (stats, vmm) = sampled?;
    assert!(
        stats.cpu_millis >= 1000,
        "the guest's work is counted: {stats:?}"
    );
    // The container also ran the launcher's network setup before it became Firecracker.
    assert!(
        stats.cpu_millis + 50 >= vmm && stats.cpu_millis <= vmm + vmm / 10 + 200,
        "{stats:?}; VMM {vmm} ms"
    );
    assert!(
        (128 << 20..=limit).contains(&stats.memory_bytes),
        "{stats:?} within {limit}"
    );
    assert!(
        (2..=64).contains(&stats.pids),
        "the VMM's threads: {stats:?}"
    );
    assert!(stats.uptime <= created.elapsed(), "{stats:?}");
    Ok(())
}

/// PERF-013's budget for one flushed small write at p95: a tool result may wait this long for `sync_before_result`.
const FLUSH_P95_BUDGET_US: u64 = 50_000;
/// The runtime's `WORKSPACE_SYNC_TIMEOUT` (60 s) in microseconds: a flush after a large write must finish well inside it.
const LARGE_FLUSH_BOUND_US: u64 = 60_000_000;

#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn perf_013_the_workspace_flush_before_a_tool_result_is_cheap() -> TestResult {
    let host = host(&Template::Probe).await?;
    let (code, report) = host.probe("flush").await?;
    assert_eq!(code, 0, "the flush probe failed inside the guest: {report}");
    let line = report
        .lines()
        .find_map(|line| line.split_once("tqa-probe: flush ").map(|(_, json)| json))
        .ok_or("no flush measurement")?;
    let summary: serde_json::Value = serde_json::from_str(line)?;
    let field =
        |series: &str, name: &str| summary[series][name].as_u64().ok_or("missing measurement");
    let (plain, flushed) = (field("plain", "p50_us")?, field("flushed", "p50_us")?);
    assert!(flushed >= plain, "a flush adds to a write: {summary}");
    assert!(
        field("flushed", "p95_us")? <= FLUSH_P95_BUDGET_US,
        "{summary}"
    );
    // The runtime's own bound for a flush (tqa-runtime WORKSPACE_SYNC_TIMEOUT, 60 s) must stay above a large write's.
    assert!(
        field("flushed_64mib", "p95_us")? < LARGE_FLUSH_BOUND_US,
        "{summary}"
    );
    std::fs::write(
        Path::new(env!("CARGO_TARGET_TMPDIR")).join("perf_013.json"),
        summary.to_string(),
    )?;
    writeln!(std::io::stdout(), "PERF-013 {summary}")?;
    Ok(())
}

// TEST: PERF-011 (its result is kept: values, images and host load, readable by perf_common.py compare)
#[test]
fn perf_011_a_result_keeps_its_values_environment_and_verdict() -> TestResult {
    let utc = perf_record::extended_utc("20260929T061502Z").ok_or("utc")?;
    // perf_common.compare parses this with strptime('%Y-%m-%dT%H:%M:%SZ').
    assert_eq!(utc, "2026-09-29T06:15:02Z");
    assert_eq!(perf_record::extended_utc("yesterday"), None);
    let result = perf_record::record(
        vec![900, 120, 140, 136, 364],
        &serde_json::json!({"utc": utc, "images": {"vmm": "sha256:v"}, "load_before": [0.5, 0.4, 0.3]}),
        &serde_json::json!({"runs": 5, "percentile": "nearest-rank"}),
    )
    .ok_or("record")?;
    assert_eq!(
        result["samples"],
        serde_json::json!([120, 136, 140, 364, 900])
    );
    // Only the millisecond values compare reads, all lower-is-better; the run count lives in config.
    assert_eq!(
        result["summary"],
        serde_json::json!({"p50_ms": 140, "p99_ms": 900, "min_ms": 120, "max_ms": 900})
    );
    assert_eq!(result["config"]["runs"], 5);
    assert_eq!(result["verdict"], "passed");
    assert_eq!(result["environment"]["images"]["vmm"], "sha256:v");
    let slow = perf_record::record(
        vec![2100, 2200, 1900],
        &serde_json::json!({}),
        &serde_json::json!({}),
    )
    .ok_or("record")?;
    assert_eq!(slow["verdict"], "failed");
    assert!(perf_record::record(vec![], &serde_json::json!({}), &serde_json::json!({})).is_none());
    assert_eq!(
        perf_record::path(Path::new("/repo"), &utc),
        Some(Path::new("/repo/tests/perf/results/2026-09-29/perf-011-061502.json").to_path_buf())
    );
    assert_eq!(
        perf_record::path(Path::new("/repo"), "20260929T061502Z"),
        None
    );
    assert_eq!(
        perf_record::load_average("0.52 0.41 0.30 2/1234 5678\n"),
        Some(serde_json::json!([0.52, 0.41, 0.30]))
    );
    assert_eq!(perf_record::load_average("garbage"), None);
    Ok(())
}

// TEST: SBX-F-027
#[tokio::test]
#[ignore = "needs KVM and host setup"]
async fn sbx_f_027_a_workspace_disk_that_wont_mount_ends_the_vm_as_an_environment_failure()
-> TestResult {
    // A real failure, with no switch in the guest: a workspace image of zeros has no ext4 superblock, so the guest
    // init's own storage mount fails, the VM powers off, and the exit record says the environment failed (126).
    let host = host(&Template::Probe).await?;
    let broken = host.storage.path().join("workspaces/zeroed.ext4");
    std::fs::create_dir_all(host.storage.path().join("workspaces"))?;
    std::fs::File::create(&broken)?.set_len(256 << 20)?;
    let (code, report) = host
        .probe_with("posture", &[], |spec| {
            spec.workspace.source = tqa_sandbox::WorkspaceSource::BlockImage(broken.clone());
        })
        .await?;
    assert_eq!(code, tqa_sandbox::ENVIRONMENT_FAILED_EXIT, "{report}");
    assert!(
        report.contains("mount storage"),
        "the console names the step that failed: {report}"
    );
    assert!(
        !report.contains("tqa-probe:"),
        "the image's entrypoint never ran: {report}"
    );
    Ok(())
}
