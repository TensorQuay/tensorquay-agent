use super::*;
use crate::{ToolContext, ToolLimits, ToolOutput, WorkspaceRoot};
use std::{os::unix::process::CommandExt as _, sync::Arc, time::Duration};
use tqa_testkit::{FakeClock, FakeSleeper};

#[tokio::test]
async fn tool_u_006_eperm_then_esrch_probe_preserves_primary_result() -> Result<(), ToolError> {
    let directory = tempfile::tempdir()?;
    let clock = FakeClock::new();
    let cx = ToolContext {
        workspace: WorkspaceRoot::open(directory.path())?,
        cancel: tokio_util::sync::CancellationToken::new(),
        clock: Arc::new(clock.clone()),
        sleeper: Arc::new(FakeSleeper::new(clock)),
        env_allow: vec![],
        limits: ToolLimits::RECOMMENDED,
    };
    let mut command = Command::new("true");
    command.process_group(0).kill_on_drop(true);
    let mut group = ProcessGroup::spawn(&mut command)?;
    group.child.wait().await?;
    group.reaped = true;
    let mut probes = [Err(Errno::EPERM), Err(Errno::ESRCH)].into_iter();
    for primary in [
        Ok(ToolOutput::text("primary output")),
        Err(ToolError::Cancelled),
    ] {
        let cleanup = group
            .wait_until_gone_with(cx.sleeper.as_ref(), |_| {
                probes.next().unwrap_or(Err(Errno::EIO))
            })
            .await;
        assert_eq!(cleanup, Ok(()));
        assert_eq!(super::super::outcome(primary.clone(), cleanup), primary);
    }
    assert!(probes.next().is_none());
    Ok(())
}

#[test]
fn tool_u_006_group_errors_preserve_all_other_failures() {
    for error in [Errno::EINVAL, Errno::EIO, Errno::EACCES] {
        assert!(members(Err(error)).is_err());
        assert!(signal_result(Err(error)).is_err());
    }
    for result in [Ok(()), Err(Errno::ESRCH), Err(Errno::EPERM)] {
        assert_eq!(signal_result(result), Ok(()));
        assert_eq!(members(result), Ok(result.is_ok()));
    }
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Real time bounds each wait because these tests observe real processes.
const OBSERVE: Duration = Duration::from_secs(3);

async fn advance_when_pending(
    clock: &FakeClock,
    sleeper: &FakeSleeper,
    graces: &[Duration],
) -> TestResult {
    for &grace in graces {
        let pending = async {
            while !sleeper.pending_durations().contains(&grace) {
                tokio::task::yield_now().await;
            }
        };
        tokio::time::timeout(OBSERVE, pending).await?;
        clock.advance(grace)?;
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_022_cleanup_reaps_exited_members_left_to_this_process() -> TestResult {
    let sleeper = FakeSleeper::new(FakeClock::new());
    let mut command = Command::new("sleep");
    command.arg("300").process_group(0).kill_on_drop(true);
    let mut group = ProcessGroup::spawn(&mut command)?;
    // A direct child in the group stands in for a member reparented to this child subreaper.
    let mut member = std::process::Command::new("true")
        .process_group(group.id.as_raw())
        .spawn()?;
    group.child.kill().await?;
    group.reaped = true;
    let wait = group.wait_until_gone_with(&sleeper, |id| killpg(id, None));
    tokio::time::timeout(OBSERVE, wait).await??;
    let reaped = member.try_wait().map_err(|error| error.raw_os_error());
    assert_eq!(reaped.err(), Some(Some(Errno::ECHILD as i32)));
    Ok(())
}

#[tokio::test]
async fn tool_u_022_finish_waits_for_killed_member_that_outlived_the_leader() -> TestResult {
    let directory = tempfile::tempdir()?;
    let ready = directory.path().join("ready");
    let clock = FakeClock::new();
    let sleeper = FakeSleeper::new(clock.clone());
    let mut command = Command::new("sh");
    let script = "(trap '' TERM; : > \"$1\"; exec sleep 300) & wait";
    command.args(["-c", script, "fixture"]).arg(&ready);
    command.process_group(0).kill_on_drop(true);
    let mut group = ProcessGroup::spawn(&mut command)?;
    let id = group.id;
    let started = async {
        while !ready.exists() {
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(OBSERVE, started).await?;
    let graces = [Duration::from_secs(2)];
    let advance = advance_when_pending(&clock, &sleeper, &graces);
    let (finished, advanced) = tokio::join!(group.finish(&sleeper), advance);
    advanced?;
    assert_eq!(finished, Ok(()));
    assert!(!members(killpg(id, None))?);
    Ok(())
}

#[tokio::test]
async fn tool_u_022_member_that_survives_sigkill_fails_cleanup() -> TestResult {
    let clock = FakeClock::new();
    let sleeper = FakeSleeper::new(clock.clone());
    let mut command = Command::new("true");
    command.process_group(0).kill_on_drop(true);
    let mut group = ProcessGroup::spawn(&mut command)?;
    let graces = [Duration::from_secs(2), Duration::from_secs(1)];
    let advance = advance_when_pending(&clock, &sleeper, &graces);
    let finish = group.finish_with(&sleeper, |_| Ok(()));
    let (finished, advanced) = tokio::join!(finish, advance);
    advanced?;
    assert_eq!(
        finished,
        Err(ToolError::Io("process group outlived SIGKILL".into()))
    );
    assert!(!group.cleaned);
    Ok(())
}
