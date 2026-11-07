use super::*;
use std::sync::Arc;
use tqa_testkit::{FakeClock, FakeSleeper};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// TEST: TOOL-U-039 (its environment check, assert_environment, holds the hook-off configuration)
#[tokio::test]
async fn rt_i_015_git_child_has_only_explicit_environment_and_safe_arguments() -> TestResult {
    let directory = tempfile::tempdir()?;
    let witness = directory.path().join("git-witness.py");
    std::fs::write(
        &witness,
        include_str!("../../tests/fixtures/git_witness.py"),
    )?;
    let repository = WorkspaceClone {
        url: "https://git.example/repo.git".into(),
        r#ref: Some("feature/example".into()),
    };
    let proxy = ProxyEndpoint::new("http://proxy.example:3128")?;
    let destination = directory.path().join("checkout");
    let input = CloneRequest {
        repository: &repository,
        destination: &destination,
        proxy: &proxy,
    };
    let clock = FakeClock::new();
    let client = GitCloner::new(Arc::new(FakeSleeper::new(clock)));
    let mut command = Command::new("python3");
    command.arg(&witness);
    command.env("GIT_CONFIG_COUNT", "1");
    command.env("TQA_CLONE_UNTRUSTED_CONFIG", "must be cleared");
    configure(&mut command, &input, None)?;
    let configured = serde_json::json!({
        "env": command.as_std().get_envs().map(|(key, value)| {
            (key.to_string_lossy().into_owned(), value.map(|value| value.to_string_lossy().into_owned()))
        }).collect::<std::collections::BTreeMap<_, _>>()
    });
    assert_environment(&configured, &proxy, directory.path())?;
    client.execute(command, &CancellationToken::new()).await?;
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(witness.with_extension("json"))?)?;
    assert_arguments(&value, &input)?;
    assert_environment(&value, &proxy, directory.path())?;
    Ok(())
}

fn assert_arguments(value: &serde_json::Value, input: &CloneRequest<'_>) -> TestResult {
    let args = value["args"].as_array().ok_or("argument witness")?;
    let args: Vec<_> = args.iter().filter_map(serde_json::Value::as_str).collect();
    for required in [
        "protocol.allow=never",
        "protocol.https.allow=always",
        "http.sslVerify=true",
        "http.followRedirects=false",
        "credential.helper=",
        "core.hooksPath=/dev/null",
        "--no-recurse-submodules",
        "--template=",
        "--branch",
        "feature/example",
    ] {
        assert!(args.contains(&required), "missing {required}");
    }
    assert_eq!(
        &args[args.len() - 3..],
        [
            "--",
            input.repository.url.as_str(),
            input.destination.to_str().ok_or("destination")?
        ]
    );
    Ok(())
}

fn assert_environment(value: &serde_json::Value, proxy: &ProxyEndpoint, home: &Path) -> TestResult {
    let env = value["env"].as_object().ok_or("environment witness")?;
    for key in ["HTTPS_PROXY", "https_proxy"] {
        assert_eq!(env[key], proxy.as_str());
    }
    for key in ["NO_PROXY", "no_proxy"] {
        assert_eq!(env[key], "");
    }
    assert_eq!(env["GIT_TERMINAL_PROMPT"], "0");
    assert_eq!(env["GIT_CONFIG_NOSYSTEM"], "1");
    assert_eq!(env["GIT_CONFIG_GLOBAL"], "/dev/null");
    assert_eq!(env["HOME"], home.to_str().ok_or("home")?);
    let permitted = [
        "PATH",
        "LANG",
        "LC_ALL",
        "HOME",
        "XDG_CONFIG_HOME",
        "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_GLOBAL",
        "GIT_TERMINAL_PROMPT",
        "GIT_ASKPASS",
        "HTTPS_PROXY",
        "https_proxy",
        "NO_PROXY",
        "no_proxy",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_CONFIG_KEY_1",
        "GIT_CONFIG_VALUE_1",
    ];
    // TOOL-U-039: the clone runs with repository hooks and the file-system monitor off (ADR-0035 §1).
    for (key, value) in crate::GIT_SAFETY_ENV {
        assert_eq!(env[key], value, "{key}");
    }
    let unexpected: Vec<_> = env
        .keys()
        .filter(|key| !permitted.contains(&key.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "unexpected environment keys: {unexpected:?}"
    );
    Ok(())
}

#[tokio::test]
async fn rt_i_015_git_classifies_only_proxy_refusal_and_bounds_diagnostics() -> TestResult {
    let clock = FakeClock::new();
    let client = GitCloner::new(Arc::new(FakeSleeper::new(clock)));
    for (message, expected) in [
        (
            "fatal: CONNECT tunnel failed, response 403",
            CloneError::EgressDenied,
        ),
        (
            "fatal: Received HTTP code 403 from proxy after CONNECT",
            CloneError::EgressDenied,
        ),
        (
            "fatal: The requested URL returned error: 403",
            CloneError::Io,
        ),
        ("fatal: certificate verification failed", CloneError::Io),
    ] {
        let mut command = Command::new("sh");
        command.args(["-c", "printf '%s' \"$1\" >&2; exit 1", "fixture", message]);
        child_io(&mut command);
        assert_eq!(
            client.execute(command, &CancellationToken::new()).await,
            Err(expected)
        );
    }
    let mut command = Command::new("sh");
    command.args([
        "-c",
        "head -c 1048576 /dev/zero >&2; printf 'CONNECT tunnel failed, response 403' >&2; exit 1",
    ]);
    child_io(&mut command);
    assert_eq!(
        client.execute(command, &CancellationToken::new()).await,
        Err(CloneError::EgressDenied)
    );
    Ok(())
}

/// How the fixture's one member reacts to TERM, which selects the cleanup path under test.
#[derive(Clone, Copy)]
enum Member {
    /// TERM stops the member, so the leader's trap completes and no KILL is needed.
    StopsOnTerm,
    /// The member ignores TERM, so the leader's trap waits until the grace ends and KILL follows.
    IgnoresTerm,
}

#[tokio::test]
async fn rt_i_015_git_cancel_and_deadline_stop_the_owned_process_group() -> TestResult {
    for timeout in [false, true] {
        for member in [Member::StopsOnTerm, Member::IgnoresTerm] {
            stop_owned_group(timeout, member).await?;
        }
    }
    Ok(())
}

async fn stop_owned_group(timeout: bool, member: Member) -> TestResult {
    let directory = tempfile::tempdir()?;
    let marker = directory.path().join("ready");
    let termed = directory.path().join("termed");
    let clock = FakeClock::new();
    let sleeper = FakeSleeper::new(clock.clone());
    let client = GitCloner::new(Arc::new(sleeper.clone()));
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let command = member_fixture(member, &marker, &termed);
    let mut task = tokio::spawn(async move { client.execute(command, &token).await });
    let stopped = async {
        let pid = child_pid(&marker).await?;
        if timeout {
            clock.advance(CLONE_TIMEOUT)?;
        } else {
            cancel.cancel();
        }
        if matches!(member, Member::IgnoresTerm) {
            grace_elapsed(&clock, &sleeper).await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(pid)
    }
    .await;
    let joined = tokio::time::timeout(Duration::from_secs(5), &mut task).await;
    let result = match joined {
        Ok(result) => result?,
        Err(elapsed) => {
            cancel.cancel();
            task.abort();
            assert!(
                task.await.is_err(),
                "an abandoned child task must be joined"
            );
            return Err(stopped.err().unwrap_or_else(|| elapsed.into()));
        }
    };
    let pid = stopped?;
    let expected = if timeout {
        CloneError::Timeout
    } else {
        CloneError::Stopped
    };
    assert_eq!(result, Err(expected));
    // Only a leader whose member stopped on TERM reaches the end of its trap before KILL.
    assert_eq!(termed.exists(), matches!(member, Member::StopsOnTerm));
    let members = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid), None);
    assert!(matches!(
        members,
        Err(nix::errno::Errno::ESRCH | nix::errno::Errno::EPERM)
    ));
    Ok(())
}

/// The member writes the leader's id only once it runs with its final TERM disposition, so a
/// signal sent after the marker appears always reaches it; the leader's trap marks a TERM exit.
fn member_fixture(member: Member, marker: &Path, termed: &Path) -> Command {
    let member = match member {
        Member::StopsOnTerm => "printf '%s' \"$1\" > \"$2\"; exec sleep 300",
        Member::IgnoresTerm => "trap '' TERM; printf '%s' \"$1\" > \"$2\"; exec sleep 300",
    };
    let leader = "trap 'wait; : > \"$2\"; exit 0' TERM; sh -c \"$3\" member \"$$\" \"$1\" & wait";
    let mut command = Command::new("sh");
    command.args(["-c", leader, "fixture"]);
    command.arg(marker).arg(termed).arg(member);
    child_io(&mut command);
    command
}

async fn grace_elapsed(clock: &FakeClock, sleeper: &FakeSleeper) -> TestResult {
    // Must equal the process group's TERM grace; the wait is bounded because a real process runs.
    const TERMINATION_GRACE: Duration = Duration::from_secs(2);
    let pending = async {
        while !sleeper.pending_durations().contains(&TERMINATION_GRACE) {
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), pending).await?;
    clock.advance(TERMINATION_GRACE)?;
    Ok(())
}

async fn child_pid(marker: &Path) -> Result<i32, tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(value) = std::fs::read_to_string(marker)
                && let Ok(pid) = value.parse()
            {
                return pid;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
}

#[test]
fn tool_u_025_a_missing_git_fails_the_clone_before_any_process_starts() {
    assert!(matches!(
        super::git_command("/nonexistent"),
        Err(CloneError::Io)
    ));
    assert!(
        super::git_command(super::GIT_PATH).is_ok(),
        "the host test image has git"
    );
}
