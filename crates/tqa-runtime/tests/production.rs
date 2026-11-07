//! RT-I-001: production clocks and process cleanup obey the same contract as injected services.
use std::{fs, time::Duration};
use tempfile::TempDir;
use tqa_runtime::{RuntimeConfig, RuntimeDeps, RuntimeInputs};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// The production reaper acts on the whole process: it kills every direct child its own reaper did not record, whichever
/// test started it. Plain `cargo test` runs these tests as threads of one process, where one test's reap killed another's
/// MCP server or fixture. Every test here holds this lock for its whole body, so a shared process behaves as one process
/// per test, as under nextest.
static PROCESS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn dependencies(directory: &TempDir) -> Result<RuntimeDeps, Box<dyn std::error::Error>> {
    let token_file = directory.path().join("credential");
    let workspace = directory.path().join("workspace");
    let state_dir = directory.path().join("state");
    fs::write(&token_file, "synthetic-runtime-credential")?;
    fs::create_dir(&workspace)?;
    fs::create_dir(&state_dir)?;
    let config = RuntimeConfig::load(RuntimeInputs {
        gateway_url: "http://gateway".into(),
        egress_proxy_url: None,
        token_file,
        workspace,
        state_dir,
        skills: directory.path().join("skills"),
        sync_before_result: false,
    })?;
    Ok(RuntimeDeps::production(config)?)
}

#[tokio::test]
async fn rt_i_001_production_timestamp_is_accepted_by_the_protocol_and_cleanup_finishes()
-> TestResult {
    let _process = PROCESS.lock().await;
    let directory = TempDir::new()?;
    let deps = dependencies(&directory)?;
    let frame = serde_json::json!({
        "type": "event", "seq": 1, "ts": deps.wall_clock.timestamp()?,
        "event": {"type": "message.user", "text": "question", "attachments": []}
    });
    let _: tqa_types::ServerFrame = serde_json::from_value(frame)?;
    // The production reaper observes operating-system child processes under a real deadline.
    tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        deps.cleanup.reap(),
    )
    .await??;
    Ok(())
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn rt_i_001_production_reaper_removes_an_escaped_child_and_its_zombie() -> TestResult {
    let _process = PROCESS.lock().await;
    let directory = TempDir::new()?;
    let deps = dependencies(&directory)?;
    // Joining the foreground shell establishes the same ownership boundary as the shell decorator.
    let mut shell = tokio::process::Command::new("sh");
    shell
        .args([
            "-c",
            "setsid sleep 1000 >/dev/null 2>&1 & printf '%s' \"$!\"",
        ])
        .kill_on_drop(true);
    let output = tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        shell.output(),
    )
    .await??;
    assert!(output.status.success());
    let pid: i32 = std::str::from_utf8(&output.stdout)?.parse()?;
    let child = std::path::PathBuf::from(format!("/proc/{pid}"));
    assert!(
        child.exists(),
        "escape fixture did not create a live descendant"
    );
    // A real process must be observed here; the cleanup itself has a one-second production bound.
    tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        deps.cleanup.reap(),
    )
    .await??;
    assert!(
        !child.exists(),
        "escaped descendant or zombie survived cleanup"
    );
    Ok(())
}

/// The reaper leaves exactly the recorded MCP server groups alone. A process started with the
/// same command line but not recorded, as a model's shell could start it, is still reaped, and the cleanup still finishes.
#[cfg(all(target_os = "linux", feature = "testing"))]
#[tokio::test]
async fn rt_i_008_the_reaper_spares_only_recorded_mcp_server_groups() -> TestResult {
    use tqa_runtime::testing::StdioServer;
    const FAKE: &str = env!("CARGO_BIN_EXE_tqa-fake-mcp");
    let _process = PROCESS.lock().await;
    let directory = TempDir::new()?;
    let deps = dependencies(&directory)?;
    let config: tqa_types::McpServer = serde_json::from_value(serde_json::json!({
        "name":"fake","transport":{"kind":"stdio","command":FAKE}
    }))?;
    let server = StdioServer::start(&config, directory.path(), Duration::from_secs(10)).await?;
    deps.cleanup.supervised().add(server.process_group());
    // The imitation: the same program and arguments, in its own group, never recorded.
    let mut imitation = tokio::process::Command::new(FAKE);
    imitation
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .process_group(0)
        .kill_on_drop(true);
    let imitation = imitation.spawn()?;
    let imitation_pid = imitation.id().ok_or("imitation pid")?;
    let imitation_proc = std::path::PathBuf::from(format!("/proc/{imitation_pid}"));
    assert!(imitation_proc.exists());
    tokio::time::timeout(
        tqa_testkit::deadline::scaled(Duration::from_secs(5)),
        deps.cleanup.reap(),
    )
    .await??;
    assert!(
        !imitation_proc.exists(),
        "the unrecorded imitation was reaped"
    );
    let answer = server
        .call(
            "echo",
            serde_json::json!({"text":"still here"}),
            Duration::from_secs(5),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await?;
    assert_eq!(answer.text, "still here", "the recorded server survived");
    deps.cleanup.supervised().remove(server.process_group());
    server.stop().await;
    Ok(())
}
