use super::*;

#[test]
fn tool_u_006_cleanup_failure_preserves_primary_error() {
    for primary in [ToolError::Timeout, ToolError::Cancelled] {
        assert_eq!(
            outcome(Err(primary.clone()), Err(ToolError::Io("cleanup".into()))),
            Err(primary)
        );
    }
}

#[test]
fn tool_u_006_cleanup_failure_replaces_only_success() {
    assert_eq!(
        outcome(
            Ok(ToolOutput::text("ok")),
            Err(ToolError::Io("cleanup".into()))
        ),
        Err(ToolError::Io("cleanup".into()))
    );
    assert_eq!(
        outcome(Ok(ToolOutput::text("ok")), Ok(())),
        Ok(ToolOutput::text("ok"))
    );
}

// Linux only: macOS reports a zombie-only group as EPERM, which already reads as no members.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn tool_u_023_exited_member_left_to_this_process_is_not_background()
-> Result<(), Box<dyn std::error::Error>> {
    use nix::{
        sys::wait::{Id, WaitPidFlag, waitid},
        unistd::Pid,
    };
    use std::os::unix::process::CommandExt as _;
    let mut command = Command::new("sh");
    command
        .args(["-c", "read line || true"])
        .stdin(Stdio::piped());
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command.process_group(0).kill_on_drop(true);
    let mut group = ProcessGroup::spawn(&mut command)?;
    let leader = i32::try_from(group.child.id().ok_or("leader id")?)?;
    // A direct child in the group stands in for a member reparented to this child subreaper.
    let mut member = std::process::Command::new("true")
        .process_group(leader)
        .spawn()?;
    let member_id = Pid::from_raw(i32::try_from(member.id())?);
    // Observe the exit without reaping it, so only this process can clear the zombie.
    let flags = WaitPidFlag::WEXITED | WaitPidFlag::WNOWAIT;
    tokio::task::spawn_blocking(move || waitid(Id::Pid(member_id), flags)).await??;
    drop(group.child.stdin.take());
    let collected = Box::pin(collect(&mut group, 4096));
    // Real time bounds the wait because a real process is observed.
    let output = tokio::time::timeout(Duration::from_secs(3), collected).await??;
    assert_eq!((output.exit_code, output.text.as_str()), (Some(0), ""));
    let reaped = member.try_wait().map_err(|error| error.raw_os_error());
    assert_eq!(reaped.err(), Some(Some(nix::errno::Errno::ECHILD as i32)));
    Ok(())
}
