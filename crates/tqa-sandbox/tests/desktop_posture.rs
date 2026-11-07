//! SBX-C-020 live on Docker (ADR-0039 section 10): a Desktop sandbox from the built Desktop image, sized as a ceiling
//! below the Desktop's own sizes leaves it. Its display, buses, window manager and VNC server run as the sandbox user;
//! nothing listens on a non-loopback address (Docker's own resolver aside); the VNC server takes no password and runs
//! with `-shared -forever -noclipboard -noprimary -seldir recv`; the sizes, the 1 GiB `/dev/shm` and the private
//! display directory are applied. The ceiling's clamp itself is the gateway's (`builtin_capabilities.rs`).
//!
//! ```sh
//! TQA_DOCKER_TESTS=1 TQA_DESKTOP_TEST_IMAGE=tqa-runtime:desktop cargo nextest run -p tqa-sandbox --run-ignored all \
//!     --test desktop_posture
//! ```
use std::{collections::BTreeMap, time::Duration};

use bollard::query_parameters::LogsOptions;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{
    ByteSize, CpuLimit, ImageRef, SandboxBackend, SandboxHandle, docker::DockerBackend,
};

#[path = "support/docker.rs"]
mod support;
use support::{TestResult, bounded, run};

/// The sizes a ceiling of 1 CPU, 2 GiB and 512 processes leaves the Desktop (its own are 2 CPUs, 4 GiB and 1,024).
const CPU_MILLICORES: u32 = 1000;
const MEMORY: u64 = 2 << 30;
const PIDS: u32 = 512;
/// The sandbox user every desktop part runs as.
const SANDBOX_UID: &str = "1000";
/// Each part of the display stack, by the program its command line starts with.
const PARTS: [&str; 5] = [
    "Xvfb",
    "dbus-daemon",
    "/usr/libexec/at-spi-bus-launcher",
    "openbox",
    "x11vnc",
];

async fn desktop(fixture: &support::Fixture) -> Result<SandboxHandle, Box<dyn std::error::Error>> {
    let selected = match std::env::var("TQA_DESKTOP_TEST_IMAGE") {
        Ok(value) if !value.is_empty() => value,
        Err(std::env::VarError::NotPresent) => "tqa-runtime:desktop".into(),
        _ => return Err("TQA_DESKTOP_TEST_IMAGE must be nonempty UTF-8".into()),
    };
    let image = bounded(fixture.docker.inspect_image(&selected))
        .await?
        .id
        .ok_or("desktop image ID missing")?;
    let mut spec = fixture.spec("desktop", true)?;
    spec.image = ImageRef::local_id(&image)?;
    assert!(fixture.backend.image_present(&spec.image).await?);
    spec.desktop = true;
    spec.gateway_url = "http://gateway.invalid".parse()?;
    spec.env = BTreeMap::from([
        (
            "PATH".into(),
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        ),
        ("HOME".into(), "/tmp".into()),
    ]);
    spec.cpu = CpuLimit::new(CPU_MILLICORES)?;
    spec.memory = ByteSize::new(MEMORY)?;
    spec.pids = PIDS;
    let backend = DockerBackend::connect(fixture.backend_config()?).await?;
    Ok(backend.create(spec, CancellationToken::new()).await?)
}

/// Every process as `<uid> <command line>`, read from /proc inside the sandbox.
async fn processes(
    fixture: &support::Fixture,
    id: &str,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let script = r#"for d in /proc/[0-9]*; do u=$(awk '/^Uid:/ {print $2}' "$d/status" 2>/dev/null); c=$(tr '\0' ' ' < "$d/cmdline" 2>/dev/null); [ -n "$c" ] && echo "$u $c"; done; true"#;
    let (exit, output) = fixture.exec(id, &["sh", "-c", script]).await?;
    assert_eq!(exit, 0, "{output}");
    Ok(output
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(uid, command)| (uid.to_owned(), command.trim().to_owned()))
        .collect())
}

/// Waits up to 30 s for every part of the display stack to be running.
async fn started(
    fixture: &support::Fixture,
    id: &str,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    for _ in 0..60 {
        let seen = processes(fixture, id).await?;
        if PARTS
            .iter()
            .all(|part| seen.iter().any(|(_, command)| command.starts_with(part)))
        {
            return Ok(seen);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(format!(
        "the display stack didn't start: {:?}",
        processes(fixture, id).await?
    )
    .into())
}

/// Every part runs as the sandbox user, and nothing in the sandbox runs as root.
fn assert_users(seen: &[(String, String)]) {
    for (uid, command) in seen {
        if PARTS.iter().any(|part| command.starts_with(part)) {
            assert_eq!(uid, SANDBOX_UID, "{command}");
        }
    }
    assert!(
        seen.iter().all(|(uid, _)| uid != "0"),
        "a process runs as root: {seen:?}"
    );
}

/// The VNC server: loopback, no password, one connection per viewer, nothing copied out.
fn assert_vnc(seen: &[(String, String)]) -> TestResult {
    let vnc = &seen
        .iter()
        .find(|(_, command)| command.starts_with("x11vnc"))
        .ok_or("x11vnc")?
        .1;
    let words: Vec<&str> = vnc.split_whitespace().collect();
    for flag in [
        "-localhost",
        "-nopw",
        "-forever",
        "-shared",
        "-noclipboard",
        "-noprimary",
    ] {
        assert!(words.contains(&flag), "{flag} missing: {vnc}");
    }
    assert!(
        words.windows(2).any(|pair| pair == ["-seldir", "recv"]),
        "-seldir recv missing: {vnc}"
    );
    for refused in ["-passwd", "-rfbauth", "-usepw", "-passwdfile"] {
        assert!(!words.contains(&refused), "{refused}: {vnc}");
    }
    Ok(())
}

/// The display's root window is white (16n C5, SBX-C-024): at a Take over the run's end reset closes the browser and
/// starts it again, and the person's live picture showed the bare root between the two. Xvfb's default root is black, a
/// black frame under "You have control" (c16k real-world A7); white is the blank page the browser comes back with.
fn assert_white_root(seen: &[(String, String)]) -> TestResult {
    let display = &seen
        .iter()
        .find(|(_, command)| command.starts_with("Xvfb"))
        .ok_or("Xvfb")?
        .1;
    let words: Vec<&str> = display.split_whitespace().collect();
    assert!(words.contains(&"-wr"), "-wr missing: {display}");
    assert!(!words.contains(&"-br"), "-br: {display}");
    Ok(())
}

/// Docker's embedded DNS server, as `/proc/net/tcp` writes its address: on every user-defined network it listens on a
/// port of its own inside the sandbox's network namespace, and the sandbox's resolv.conf names it.
const DOCKER_RESOLVER: &str = "0B00007F";
/// The same address as resolv.conf writes it.
const DOCKER_RESOLVER_ADDRESS: std::net::Ipv4Addr = std::net::Ipv4Addr::new(127, 0, 0, 11);

/// Every listening socket is on loopback (`127.0.0.1` or `::1`, never a wildcard or another address), the VNC server's
/// 5900 among them. The one exception is Docker's resolver, and only when resolv.conf names it.
async fn assert_loopback(fixture: &support::Fixture, id: &str) -> TestResult {
    let (exit, sockets) = fixture
        .exec(
            id,
            &["sh", "-c", "cat /proc/net/tcp /proc/net/tcp6 2>/dev/null"],
        )
        .await?;
    assert_eq!(exit, 0, "{sockets}");
    let listening: Vec<&str> = sockets
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields.get(3) == Some(&"0A")).then(|| fields[1])
        })
        .collect();
    assert!(
        listening.iter().any(|local| local.ends_with(":170C")),
        "the VNC server isn't listening on 5900: {sockets}"
    );
    let (exit, resolv) = fixture.exec(id, &["cat", "/etc/resolv.conf"]).await?;
    assert_eq!(exit, 0, "{resolv}");
    let named = DOCKER_RESOLVER_ADDRESS.to_string();
    let resolver = resolv
        .lines()
        .any(|line| line.split_whitespace().eq(["nameserver", named.as_str()]));
    for local in &listening {
        let address = local.split(':').next().unwrap_or_default();
        assert!(
            address == "0100007F"
                || address == "00000000000000000000000001000000"
                || (resolver && address == DOCKER_RESOLVER),
            "listening off loopback: {local}"
        );
    }
    Ok(())
}

/// The runtime's program, which desktop-init starts only once the VNC server's warm-up is over.
const RUNTIME: &str = "/usr/local/bin/tqa-runtime";

/// Once the runtime has started, the VNC server has no client and has its selection window: desktop-init's warm-up
/// viewer (ADR-0039 section 6: x11vnc makes that window only some seconds into a connection) is gone before the runtime
/// starts, so no person's viewer ever shares the server with it, and a paste reaches the clipboard from the first. A
/// client is an established connection on 5900 (`:170C`).
async fn assert_vnc_unshared(fixture: &support::Fixture, id: &str) -> TestResult {
    let mut started = false;
    for _ in 0..80 {
        if processes(fixture, id)
            .await?
            .iter()
            .any(|(_, command)| command.starts_with(RUNTIME))
        {
            started = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(
        started,
        "the runtime didn't start after the VNC server's warm-up"
    );
    let (exit, sockets) = fixture
        .exec(
            id,
            &["sh", "-c", "cat /proc/net/tcp /proc/net/tcp6 2>/dev/null"],
        )
        .await?;
    assert_eq!(exit, 0, "{sockets}");
    let clients: Vec<&str> = sockets
        .lines()
        .filter(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            fields.get(3) == Some(&"01")
                && fields.get(1).is_some_and(|local| local.ends_with(":170C"))
        })
        .collect();
    assert!(
        clients.is_empty(),
        "the VNC server has a client once the runtime runs: {clients:?}"
    );
    // And it did its job: the server's selection window is there (nameless, 1x1, at +3+2, as x11vnc makes it).
    let (exit, windows) = fixture
        .exec(id, &["env", "DISPLAY=:0", "xwininfo", "-root", "-children"])
        .await?;
    assert_eq!(exit, 0, "{windows}");
    assert!(
        windows
            .lines()
            .any(|line| line.contains("(has no name): ()") && line.contains(" 1x1+3+2 ")),
        "the VNC server's selection window isn't there once the runtime runs: {windows}"
    );
    Ok(())
}

/// The sizes as given, the 1 GiB shared memory, and the display's and browser's directories private to the user.
async fn assert_applied(fixture: &support::Fixture, id: &str) -> TestResult {
    let host = bounded(fixture.docker.inspect_container(id, None))
        .await?
        .host_config
        .ok_or("missing host configuration")?;
    assert_eq!(host.memory, Some(i64::try_from(MEMORY)?));
    // The CPU as the Docker backend gives it (`docker/config.rs`): a quota in microseconds of each 1 s period, a
    // thousand per millicore, not `NanoCpus`.
    assert_eq!(
        (host.cpu_quota, host.cpu_period),
        (Some(i64::from(CPU_MILLICORES) * 1000), Some(1_000_000))
    );
    assert_eq!(host.pids_limit, Some(i64::from(PIDS)));
    assert_eq!(host.shm_size, Some(1 << 30));
    assert_eq!(
        fixture
            .exec(
                id,
                &[
                    "stat",
                    "-c",
                    "%u %a",
                    "/run/tqa-display",
                    "/run/tqa-browser"
                ]
            )
            .await?,
        (0, "1000 700\n1000 700\n".into())
    );
    Ok(())
}

/// How long the display stack runs, once up, before its log is read for a part started again (`keep`'s line).
const SETTLE: Duration = Duration::from_secs(8);

/// The lines of the init's log that say a part exited and was started again (desktop-init.sh's `keep`).
fn restarts(log: &str) -> Vec<&str> {
    log.lines()
        .filter(|line| {
            line.contains("tqa-desktop-init: ") && line.ends_with("exited; starting it again")
        })
        .collect()
}

/// No part of the display stack restarts in a loop (c16: the accessibility launcher exited and was started again every
/// second for the sandbox's life): after `SETTLE`, the sandbox's log has no restart line, and one accessibility
/// launcher runs, the one the session bus activated.
async fn assert_no_restart_loop(fixture: &support::Fixture, id: &str) -> TestResult {
    tokio::time::sleep(SETTLE).await;
    let options = LogsOptions {
        stdout: true,
        stderr: true,
        ..Default::default()
    };
    let mut stream = fixture.docker.logs(id, Some(options));
    let mut log = String::new();
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(30), stream.next()).await? {
        log.push_str(&String::from_utf8_lossy(&chunk?.into_bytes()));
    }
    assert_eq!(
        restarts(&log),
        Vec::<&str>::new(),
        "a part of the display stack restarts:\n{log}"
    );
    let launchers: Vec<String> = processes(fixture, id)
        .await?
        .into_iter()
        .map(|(_, command)| command)
        .filter(|command| command.starts_with("/usr/libexec/at-spi-bus-launcher"))
        .collect();
    assert_eq!(
        launchers.len(),
        1,
        "one accessibility launcher: {launchers:?}"
    );
    Ok(())
}

#[test]
fn sbx_c_020_a_restart_line_is_found_and_nothing_else_is() {
    let log = "tqa-desktop-init: accessibility exited; starting it again\n\
               some runtime line\n\
               tqa-desktop-init: the display didn't start\n\
               tqa-desktop-init: vnc exited; starting it again\n";
    assert_eq!(
        restarts(log),
        [
            "tqa-desktop-init: accessibility exited; starting it again",
            "tqa-desktop-init: vnc exited; starting it again"
        ]
    );
    assert!(restarts("tqa-desktop-init: the accessibility bus didn't start").is_empty());
}

// TEST: SBX-C-020
// TEST: SBX-C-024
#[tokio::test]
#[ignore = "needs Docker, TQA_DOCKER_TESTS=1 and the Desktop image (TQA_DESKTOP_TEST_IMAGE)"]
async fn sbx_c_020_the_desktop_runs_as_the_sandbox_user_on_loopback_with_its_vnc_flags()
-> TestResult {
    run(async |fixture| {
        let handle = desktop(fixture).await?;
        let seen = started(fixture, &handle.id).await?;
        assert_users(&seen);
        assert_vnc(&seen)?;
        assert_white_root(&seen)?;
        assert_loopback(fixture, &handle.id).await?;
        assert_vnc_unshared(fixture, &handle.id).await?;
        assert_applied(fixture, &handle.id).await?;
        assert_no_restart_loop(fixture, &handle.id).await
    })
    .await
}
