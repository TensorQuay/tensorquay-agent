//! SEC-010: a sandbox that spawns processes without bound stops at its pids limit, and nothing outside it notices.
//! The storm is bounded and gentle: background sleeps past the limit, each refused spawn counted and never retried,
//! so nothing spins (no artificial CPU or memory load on the machine that runs it). It runs in Python in the
//! runtime image, as on the microVM (`deploy/firecracker/pids_checks.py`): a shell loop aborts at its first refused
//! fork, so it would stop at the limit instead of pushing past it. While the storm holds the limit, the checks run.

use std::{collections::BTreeMap, time::Duration};

use tokio_util::sync::CancellationToken;
use tqa_sandbox::{ByteSize, CpuLimit, ImageRef, SandboxBackend, SandboxHandle};

#[path = "support/docker.rs"]
mod support;
use support::{TestResult, bounded, run};

/// The limit this test gives the stormed sandbox.
const PIDS: u32 = 64;
const PAST_THE_LIMIT: u32 = 100;
/// As on the microVM: long enough for the neighbour's command, and over before the sandbox's own runtime, whose gateway
/// this test does not provide, might give up and end the container.
const SLEEP_SECONDS: u64 = 20;
/// The storm reads its limit from its own cgroup, spawns the limit plus 100 sleeps, prints one report line and reaps.
const STORM: &str = r#"import errno, os
g = "/sys/fs/cgroup"
limit = int(open(g + "/pids.max").read())
storm = limit + PAST
children, refused = [], 0
for _ in range(storm):
    try:
        children.append(os.posix_spawn("/bin/sleep", ["sleep", SLEEP], {}))
    except OSError as error:
        if error.errno != errno.EAGAIN:
            raise
        refused += 1
current = int(open(g + "/pids.current").read())
events = dict(line.split() for line in open(g + "/pids.events"))
print("pids max={} storm={} current={} limited={} refused={}".format(
    limit, storm, current, events.get("max", 0), refused), flush=True)
for child in children:
    os.waitpid(child, 0)
print("pids done", flush=True)
"#;

fn storm() -> String {
    STORM
        .replace("PAST", &PAST_THE_LIMIT.to_string())
        .replace("SLEEP", &format!("\"{SLEEP_SECONDS}\""))
}

/// `pids max=… storm=… current=… limited=… refused=…` as numbers, from the storm's output.
fn report(output: &str) -> Option<BTreeMap<String, u64>> {
    let line = output.lines().find(|line| line.starts_with("pids max="))?;
    let fields: BTreeMap<String, u64> = line
        .split_whitespace()
        .skip(1)
        .map(|field| {
            let (key, value) = field.split_once('=')?;
            Some((key.to_owned(), value.parse().ok()?))
        })
        .collect::<Option<_>>()?;
    ["max", "storm", "current", "limited", "refused"]
        .iter()
        .all(|key| fields.contains_key(*key))
        .then_some(fields)
}

#[test]
fn sec_010_the_storm_reads_its_limit_and_its_report_parses() {
    let storm = storm();
    assert!(storm.contains("storm = limit + 100"), "{storm}");
    assert!(storm.contains(r#"["sleep", "20"]"#), "{storm}");
    let parsed =
        report("noise\npids max=64 storm=164 current=64 limited=101 refused=101\npids done\n");
    let expected: BTreeMap<String, u64> = [
        ("max", 64),
        ("storm", 164),
        ("current", 64),
        ("limited", 101),
        ("refused", 101),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    assert_eq!(parsed, Some(expected));
    assert_eq!(report("pids max=64\n"), None);
}

async fn runtime_image(fixture: &support::Fixture) -> Result<ImageRef, Box<dyn std::error::Error>> {
    let selected = match std::env::var("TQA_RUNTIME_TEST_IMAGE") {
        Ok(value) if !value.is_empty() => value,
        Err(std::env::VarError::NotPresent) => "tqa-runtime:workstation".into(),
        _ => return Err("TQA_RUNTIME_TEST_IMAGE must be nonempty UTF-8".into()),
    };
    let image = bounded(fixture.docker.inspect_image(&selected))
        .await?
        .id
        .ok_or("runtime image ID missing")?;
    Ok(ImageRef::local_id(&image)?)
}

/// From the host, without a process in the sandbox: the count reaches the limit and never passes it.
async fn reaches_the_limit(
    fixture: &support::Fixture,
    stormed: &SandboxHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut highest = 0;
    loop {
        let pids = fixture.backend.stats(stormed).await?.pids;
        assert!(
            pids <= u64::from(PIDS),
            "{pids} processes exceed the limit of {PIDS}"
        );
        highest = highest.max(pids);
        if pids >= u64::from(PIDS) - 2 {
            return Ok(());
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the storm never reached the limit ({highest} processes)"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// The storm's output once its sleeps have ended, read from the bound workspace on the host: an exec needs a process
/// in the sandbox, and one at its limit (the runtime's own threads count too) may refuse it however long the test waits.
async fn finished_storm(
    fixture: &support::Fixture,
    stormed: &SandboxHandle,
) -> Result<String, Box<dyn std::error::Error>> {
    let path = fixture.workspace.path().join("storm.out");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(SLEEP_SECONDS + 60);
    loop {
        if let Ok(output) = std::fs::read_to_string(&path)
            && output.contains("pids done")
        {
            return Ok(output);
        }
        if tokio::time::Instant::now() >= deadline {
            // Whether the container itself ended (its runtime gave up) or the storm was stuck in it.
            let state = bounded(fixture.docker.inspect_container(&stormed.id, None))
                .await
                .map_or_else(
                    |error| format!("inspect failed: {error}"),
                    |inspect| format!("{:?}", inspect.state),
                );
            return Err(format!(
                "the storm never finished: {}; container state: {state}",
                std::fs::read_to_string(&path).unwrap_or_default()
            )
            .into());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[tokio::test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1 and a Workstation runtime image; run with --run-ignored all"]
async fn sec_010_a_process_storm_stops_at_the_pids_limit_and_other_sandboxes_are_unaffected()
-> TestResult {
    run(async |fixture| {
        // The runtime image's own settings, as runtime_posture.rs gives them: without them its runtime exits at once.
        // The fixture's host directory as its workspace: the storm's report is read from there (finished_storm).
        let mut spec = fixture.spec("pids-storm", true)?;
        spec.image = runtime_image(fixture).await?;
        spec.gateway_url = "http://gateway.invalid".parse()?;
        spec.env = BTreeMap::from([
            (
                "PATH".into(),
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
            ),
            ("HOME".into(), "/tmp".into()),
        ]);
        spec.cpu = CpuLimit::new(1000)?;
        spec.memory = ByteSize::new(512 * 1024 * 1024)?;
        spec.pids = PIDS;
        let stormed = fixture
            .backend
            .create(spec, CancellationToken::new())
            .await?;
        let neighbour = fixture
            .backend
            .create(
                fixture.spec("pids-neighbour", false)?,
                CancellationToken::new(),
            )
            .await?;
        // In the background: the exec's shell starts the storm and returns.
        let command = [
            "sh",
            "-c",
            "umask 022; python3 -c \"$0\" > /workspace/storm.out 2>&1 &",
            &storm(),
        ];
        let started = fixture.exec(&stormed.id, &command).await?;
        assert_eq!(started.0, 0, "the storm did not start: {}", started.1);
        reaches_the_limit(fixture, &stormed).await?;
        // While the storm holds the limit, another sandbox still runs a command.
        assert_eq!(
            fixture.exec(&neighbour.id, &["true"]).await?.0,
            0,
            "another sandbox must still run a command during the storm"
        );
        let holding = fixture.backend.stats(&stormed).await?.pids;
        assert!(
            (u64::from(PIDS) - 2..=u64::from(PIDS)).contains(&holding),
            "the storm no longer held the limit when the neighbour ran ({holding} processes)"
        );
        // The storm's own report: it pushed past the limit and was refused, not aborted.
        let output = finished_storm(fixture, &stormed).await?;
        let numbers = report(&output).ok_or_else(|| format!("no storm report: {output}"))?;
        assert_eq!(numbers["max"], u64::from(PIDS), "{output}");
        assert_eq!(
            numbers["storm"],
            u64::from(PIDS + PAST_THE_LIMIT),
            "{output}"
        );
        assert!(numbers["current"] <= u64::from(PIDS), "{output}");
        // The storm is the limit plus PAST_THE_LIMIT, so at least that many spawns were refused at the limit.
        assert!(
            numbers["refused"] >= u64::from(PAST_THE_LIMIT) && numbers["limited"] > 0,
            "{output}"
        );
        // A sandbox whose storm reached its limit can still be destroyed, and its processes go with it.
        fixture.backend.destroy(&stormed).await?;
        let gone = fixture.docker.inspect_container(&stormed.id, None).await;
        assert!(
            matches!(
                gone,
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404,
                    ..
                })
            ),
            "the stormed sandbox is still there after destroy: {gone:?}"
        );
        fixture.backend.destroy(&neighbour).await?;
        Ok(())
    })
    .await
}
