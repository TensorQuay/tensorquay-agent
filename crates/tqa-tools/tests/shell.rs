//! TOOL-U-006/007/039 and TOOL-E-009: owned process groups, bounded shell output and the hook-off git configuration.
#[path = "support/context.rs"]
mod support;
use serde_json::json;
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    process::Command,
    time::{Duration, Instant},
};
use tqa_testkit::workspace_fixture;
use tqa_tools::{Tool, ToolError, builtin::Shell};

type TestResult = Result<(), Box<dyn Error>>;

async fn wait_for_file(path: &std::path::Path) -> Result<String, Box<dyn Error>> {
    Ok(tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(text) = tokio::fs::read_to_string(path).await
                && !text.is_empty()
            {
                return text;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?)
}

fn group_exists(group: &str) -> Result<bool, Box<dyn Error>> {
    let status = Command::new("pgrep").args(["-g", group.trim()]).status()?;
    assert!(matches!(status.code(), Some(0 | 1)));
    Ok(status.success())
}

#[tokio::test]
async fn tool_u_006_exit_codes_cwd_and_stream_order() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let tool = Shell::new();
    let output = tool
        .call(json!({"command":"pwd; exit 7", "cwd":"nested"}), &cx)
        .await?;
    assert_eq!(output.exit_code, Some(7));
    assert_eq!(
        output.text.trim(),
        fixture.root().join("nested").display().to_string()
    );
    let run = tool.call(json!({"command":"printf 'out-one\n'; printf 'err-one\n' >&2; printf ready > ready; while [ ! -f proceed ]; do :; done; printf 'out-two\n'; printf 'err-two\n' >&2"}), &cx);
    let handshake = async {
        wait_for_file(&fixture.root().join("ready")).await?;
        fs::write(fixture.root().join("proceed"), "go")?;
        Ok::<_, Box<dyn Error>>(())
    };
    let (output, handshake) = tokio::join!(run, handshake);
    handshake?;
    let output = output?.text;
    assert!(output.find("out-one") < output.find("out-two"));
    assert!(output.find("[stderr] err-one") < output.find("[stderr] err-two"));
    assert!(output.contains("[stderr] err-one\n") && output.contains("[stderr] err-two\n"));
    Ok(())
}

#[tokio::test]
async fn tool_u_006_timeout_kills_grandchild_group() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    cx.limits.shell_timeout = Duration::from_millis(150);
    let result = Shell::new()
        .call(
            json!({"command":"trap 'wait; exit' TERM; sleep 30 & printf '%s' $$ > group; wait"}),
            &cx,
        )
        .await;
    let group = fs::read_to_string(fixture.root().join("group"))?;
    let alive = group_exists(&group)?;
    if alive {
        nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(group.trim().parse()?),
            nix::sys::signal::Signal::SIGKILL,
        )?;
    }
    assert!(matches!(result, Err(ToolError::Timeout)));
    assert!(!alive, "grandchild survived timeout");
    Ok(())
}

#[tokio::test]
async fn tool_u_006_cancellation_signals_the_whole_group_and_reaps_it() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    let tool = Shell::new();
    // A member that writes `got_term` when SIGTERM reaches it. Cancellation that signalled only the shell would leave it
    // running until SIGKILL after the termination grace (2 s): a timing bound can't tell that apart from a loaded
    // host's prompt cancel, the marker can.
    let call = tool.call(
        json!({"command":"trap 'wait; exit' TERM; \
            sh -c 'trap \"printf term > got_term; exit 0\" TERM; sleep 30 & wait' & printf '%s' $$ > group; wait"}),
        &cx,
    );
    let cancel = async {
        let group = wait_for_file(&fixture.root().join("group")).await?;
        // Wait for the grandchild to finish exec before testing cancellation during sleep.
        tokio::time::timeout(Duration::from_secs(3), wait_for_sleep(&group)).await??;
        let start = Instant::now();
        cx.cancel.cancel();
        Ok::<_, Box<dyn Error>>((group, start))
    };
    let (result, cancel) = tokio::join!(call, cancel);
    let (group, start) = cancel?;
    assert!(matches!(result, Err(ToolError::Cancelled)));
    assert!(
        fixture.root().join("got_term").is_file(),
        "SIGTERM reached every member of the group, not only the shell"
    );
    assert!(!group_exists(&group)?);
    // A hang guard only: the sleep's 30 s is the failure this bounds.
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_006_caps_output_and_marks_lossy_binary() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    cx.limits.shell_output_bytes = 64;
    let output = Shell::new().call(json!({"command":"printf HEAD; i=0; while [ $i -lt 200 ]; do printf x; i=$((i+1)); done; printf TAIL"}), &cx).await?.text;
    assert!(output.starts_with("HEAD"));
    assert!(output.ends_with("TAIL"));
    assert!(output.contains("[truncated]"));
    assert!(output.len() <= 64);
    let output = Shell::new()
        .call(json!({"command":"printf '\\377'"}), &cx)
        .await?
        .text;
    assert!(output.contains("[lossy UTF-8]"));
    assert!(output.contains('\u{fffd}'));
    Ok(())
}

#[tokio::test]
async fn tool_u_007_environment_is_scrubbed_and_allow_listed() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    const MARKER: &str = "TQA_TOOL_TEST_ENV_CHILD";
    if std::env::var_os(MARKER).is_none() {
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "tool_u_007_environment_is_scrubbed_and_allow_listed",
            ])
            .env(MARKER, "1")
            .env("FOO_TOKEN", "fixture-value")
            .env("AWS_SECRET", "fixture-value")
            .env("AWS_SECRET_ACCESS_KEY", "fixture-value")
            .env("TQA_TOOL_TEST_ALLOWED", "fixture-allowed")
            .env("GIT_CONFIG_COUNT", "0")
            .env("LANG", "C")
            .env("TERM", "dumb")
            .output()?;
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        // A filter that matches nothing also exits successfully; the child must have run this test.
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("test result: ok. 1 passed;"), "{stdout}");
        return Ok(());
    }
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    cx.env_allow.push("TQA_TOOL_TEST_ALLOWED".into());
    cx.env_allow.push("HOME".into());
    // TEST: TOOL-U-039
    // TOOL-U-039: a passed-through GIT_CONFIG_COUNT never undoes the hook-off configuration.
    cx.env_allow.push("GIT_CONFIG_COUNT".into());
    let output = Shell::new().call(json!({"command":"env"}), &cx).await?;
    assert_eq!(output.exit_code, Some(0));
    assert_allowed_environment(&output.text, fixture.root())?;
    let home = fixture.root().join(".tqa/home");
    assert_eq!(fs::metadata(&home)?.permissions().mode() & 0o777, 0o700);
    assert!(home.is_dir());
    Ok(())
}

fn assert_allowed_environment(text: &str, root: &std::path::Path) -> TestResult {
    let mut observed: BTreeMap<_, _> = text
        .lines()
        .map(|line| line.split_once('=').ok_or("invalid environment entry"))
        .collect::<Result<_, _>>()?;
    // Shells may create these process metadata entries even after env_clear.
    if let Some(value) = observed.remove("PWD") {
        assert_eq!(value, root.to_string_lossy());
    }
    if let Some(value) = observed.remove("SHLVL") {
        assert!(matches!(value, "0" | "1"), "unexpected shell depth");
    }
    if let Some(value) = observed.remove("_") {
        assert!(value == "env" || value.ends_with("/env"));
    }
    let path = std::env::var("PATH")?;
    let home = root.join(".tqa/home");
    let home = home.to_string_lossy();
    let mut expected = BTreeMap::from([
        ("HOME", home.as_ref()),
        ("PATH", path.as_str()),
        ("LANG", "C"),
        ("TERM", "dumb"),
        ("TQA_TOOL_TEST_ALLOWED", "fixture-allowed"),
    ]);
    expected.extend(tqa_tools::GIT_SAFETY_ENV);
    assert_eq!(
        observed.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>(),
        "unexpected child environment names"
    );
    assert!(
        observed == expected,
        "allowed child environment values differ"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_007_shell_home_refuses_symlinks_and_regular_files() -> TestResult {
    use std::os::unix::fs::symlink;
    let fixture = workspace_fixture()?;
    let outside = tempfile::tempdir()?;
    let cx = support::context(&fixture);
    fs::create_dir(fixture.root().join(".tqa"))?;
    let home = fixture.root().join(".tqa/home");
    symlink(outside.path(), &home)?;
    assert!(
        Shell::new()
            .call(json!({"command":"printf ran > unexpected"}), &cx)
            .await
            .is_err()
    );
    fs::remove_file(&home)?;
    symlink(fixture.root().join("nested"), &home)?;
    assert!(
        Shell::new()
            .call(json!({"command":"printf ran > unexpected"}), &cx)
            .await
            .is_err()
    );
    fs::remove_file(&home)?;
    fs::write(&home, "not a directory")?;
    assert!(
        Shell::new()
            .call(json!({"command":"printf ran > unexpected"}), &cx)
            .await
            .is_err()
    );
    assert!(!fixture.root().join("unexpected").exists());
    assert!(!outside.path().join("home").exists());
    Ok(())
}

#[tokio::test]
async fn tool_u_006_invalid_cwd_limits_environment_and_precancel() -> TestResult {
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    for args in [
        json!({"command":"true","cwd":"../outside"}),
        json!({"command":"true","cwd":"hello.txt"}),
        json!({"command":"true","timeout_ms":0}),
        json!({"command":42}),
    ] {
        assert!(Shell::new().call(args, &cx).await.is_err());
    }
    cx.env_allow.push("bad=name".into());
    assert!(matches!(
        Shell::new().call(json!({"command":"true"}), &cx).await,
        Err(ToolError::InvalidArgs { .. })
    ));
    cx.env_allow.clear();
    cx.cancel.cancel();
    assert!(matches!(
        Shell::new()
            .call(json!({"command":"printf bad > unwanted"}), &cx)
            .await,
        Err(ToolError::Cancelled)
    ));
    assert!(!fixture.root().join("unwanted").exists());
    Ok(())
}

#[tokio::test]
async fn tool_u_006_ignored_term_escalates_with_injected_grace() -> TestResult {
    use std::sync::Arc;
    use tqa_testkit::{FakeClock, FakeSleeper};
    let fixture = workspace_fixture()?;
    let mut cx = support::context(&fixture);
    let clock = FakeClock::new();
    let sleeper = Arc::new(FakeSleeper::new(clock.clone()));
    cx.clock = Arc::new(clock.clone());
    cx.sleeper = sleeper.clone();
    let tool = Shell::new();
    let call = tool.call(
        json!({"command":"trap '' TERM; printf '%s' $$ > group; while :; do :; done"}),
        &cx,
    );
    let advance = async {
        let group = wait_for_file(&fixture.root().join("group")).await?;
        cx.cancel.cancel();
        tokio::time::timeout(Duration::from_secs(3), wait_for_grace(&sleeper)).await?;
        clock.advance(Duration::from_secs(2))?;
        Ok::<_, Box<dyn Error>>(group)
    };
    let (result, advance) = tokio::join!(call, advance);
    assert!(matches!(result, Err(ToolError::Cancelled)));
    assert!(!group_exists(&advance?)?);
    Ok(())
}

async fn wait_for_sleep(group: &str) -> Result<(), std::io::Error> {
    loop {
        let found = tokio::process::Command::new("pgrep")
            .args(["-g", group.trim(), "-x", "sleep"])
            .output()
            .await?;
        if found.status.success() {
            return Ok(());
        }
        tokio::task::yield_now().await;
    }
}
async fn wait_for_grace(sleeper: &tqa_testkit::FakeSleeper) {
    while !sleeper.durations().contains(&Duration::from_secs(2)) {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn tool_u_006_background_children_stop_when_leader_returns() -> TestResult {
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    for redirect in ["", "> /dev/null 2>&1"] {
        let command =
            format!("printf '%s' $$ > background_group; echo started; sleep 20 {redirect} &");
        let start = Instant::now();
        let output = Shell::new().call(json!({"command":command}), &cx).await?;
        // Waiting for the background child would take its 20 s; 10 s tells the two apart on any host (1 s was close
        // to a loaded host's process start).
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(output.exit_code, Some(0));
        assert!(output.text.starts_with("started\n"));
        assert!(
            output
                .text
                .ends_with("[background processes were stopped when the command returned]")
        );
        assert!(!group_exists(&fs::read_to_string(
            fixture.root().join("background_group")
        )?)?);
    }
    Ok(())
}

// TEST: TOOL-U-064
#[tokio::test]
async fn tool_u_064_on_a_desktop_the_shell_points_screen_input_to_the_computer_tool() -> TestResult
{
    // c16k real-world A10 and A12b (17 Oct 2026): the model drove its desktop with xdotool from the shell, outside the
    // computer tool's action cap, settle check and step pictures. Opening an application from the shell stays fine; a
    // command that names an X input tool still runs, and its result says where screen input belongs.
    const NOTE: &str = "[the desktop's screen is the computer tool's: input sent from the shell isn't checked, \
                        counted or shown to the person]";
    let on_desktop = Shell::new().with_desktop();
    let description = &on_desktop.spec().description;
    assert!(
        description.contains("act on the screen with the computer tool"),
        "{description}"
    );
    assert!(!Shell::new().spec().description.contains("computer tool"));
    let fixture = workspace_fixture()?;
    let cx = support::context(&fixture);
    for command in [
        "echo xdotool search --name Open",
        "echo DISPLAY=:0 /usr/libexec/tqa/xdotool key Return",
        "echo 'import pyautogui'",
        "echo ydotool; echo xte",
    ] {
        let output = on_desktop.call(json!({"command": command}), &cx).await?;
        assert_eq!(output.exit_code, Some(0), "it still runs: {command}");
        assert!(output.text.ends_with(NOTE), "{command}: {}", output.text);
    }
    for command in [
        "echo xdotool.md",
        "echo xdotoolbox",
        "echo mousepad notes.txt",
    ] {
        let output = on_desktop.call(json!({"command": command}), &cx).await?;
        assert!(!output.text.contains(NOTE), "{command}: {}", output.text);
    }
    let elsewhere = Shell::new()
        .call(json!({"command": "echo xdotool key a"}), &cx)
        .await?;
    assert!(
        !elsewhere.text.contains(NOTE),
        "no desktop: {}",
        elsewhere.text
    );
    Ok(())
}
