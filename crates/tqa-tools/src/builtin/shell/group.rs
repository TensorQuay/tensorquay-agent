//! Process group ownership survives early returns and dropped call futures.
use crate::ToolError;
use nix::{
    errno::Errno,
    sys::{
        signal::{Signal, killpg},
        wait::{WaitPidFlag, WaitStatus, waitpid},
    },
    unistd::Pid,
};
use std::time::Duration;
use tokio::process::{Child, Command};
use tqa_types::Sleeper;

pub(crate) struct ProcessGroup {
    pub(crate) child: Child,
    id: Pid,
    pub(crate) reaped: bool,
    cleaned: bool,
}
impl ProcessGroup {
    pub(crate) fn spawn(command: &mut Command) -> Result<Self, ToolError> {
        let child = command.spawn()?;
        let id = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .ok_or_else(|| ToolError::Io("process id missing".into()))?;
        Ok(Self {
            child,
            id: Pid::from_raw(id),
            reaped: false,
            cleaned: false,
        })
    }
    fn signal(&self, signal: Signal) -> Result<(), ToolError> {
        signal_result(killpg(self.id, signal))
    }
    /// Once the leader is reaped, members this process adopted are reaped first, so an exited
    /// member is never reported as a running background process.
    pub(super) fn has_members(&self) -> Result<bool, ToolError> {
        if self.reaped {
            self.reap_adopted_members()?;
        }
        members(killpg(self.id, None))
    }
    async fn wait_until_gone_with(
        &mut self,
        sleeper: &dyn Sleeper,
        mut probe: impl FnMut(Pid) -> Result<(), Errno>,
    ) -> Result<(), ToolError> {
        const GROUP_POLL_INTERVAL: Duration = Duration::from_millis(5);
        loop {
            if !self.reaped && self.child.try_wait()?.is_some() {
                self.reaped = true;
            }
            if self.reaped {
                self.reap_adopted_members()?;
            }
            if !members(probe(self.id))? {
                return Ok(());
            }
            tokio::select! {
                () = sleeper.sleep(GROUP_POLL_INTERVAL) => {},
                () = tokio::time::sleep(GROUP_POLL_INTERVAL) => {},
            }
        }
    }
    /// Linux keeps an exited member visible to `killpg` until its parent reaps it. Members that
    /// outlive the leader are reparented to the nearest child subreaper, which is this process when
    /// the sandbox runtime hosts the tool, so nobody else reaps them while cleanup waits.
    /// Waiting starts only after the leader is reaped, so the asynchronous process driver keeps
    /// sole ownership of the leader.
    fn reap_adopted_members(&self) -> Result<(), ToolError> {
        // Bounds one poll when members keep forking; the next poll continues.
        const REAPS_PER_POLL: usize = 64;
        for _ in 0..REAPS_PER_POLL {
            match waitpid(Pid::from_raw(-self.id.as_raw()), Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::StillAlive) | Err(Errno::ECHILD) => return Ok(()),
                Ok(_) | Err(Errno::EINTR) => {}
                Err(error) => {
                    return Err(ToolError::Io(format!("group reap failed: {error}")));
                }
            }
        }
        Ok(())
    }
    pub(crate) async fn finish(&mut self, sleeper: &dyn Sleeper) -> Result<(), ToolError> {
        self.finish_with(sleeper, |id| killpg(id, None)).await
    }
    async fn finish_with(
        &mut self,
        sleeper: &dyn Sleeper,
        mut probe: impl FnMut(Pid) -> Result<(), Errno>,
    ) -> Result<(), ToolError> {
        const TERMINATION_GRACE: Duration = Duration::from_secs(2);
        // SIGKILL cannot be caught, so this only bounds exit and reaping on a loaded host, or a
        // member stuck in uninterruptible sleep.
        const KILL_GRACE: Duration = Duration::from_secs(1);
        self.signal(Signal::SIGTERM)?;
        tokio::select! {
            result = self.wait_until_gone_with(sleeper, &mut probe) => { result?; }
            () = sleeper.sleep(TERMINATION_GRACE) => {}
            () = tokio::time::sleep(TERMINATION_GRACE) => {}
        }
        self.signal(Signal::SIGKILL)?;
        if !self.reaped {
            self.child.wait().await?;
            self.reaped = true;
        }
        // A killed member still exists until it is reaped; returning earlier would report a
        // stopped group while a member is still visible to the host.
        let survived = || ToolError::Io("process group outlived SIGKILL".into());
        tokio::select! {
            result = self.wait_until_gone_with(sleeper, &mut probe) => { result?; }
            () = sleeper.sleep(KILL_GRACE) => return Err(survived()),
            () = tokio::time::sleep(KILL_GRACE) => return Err(survived()),
        }
        self.cleaned = true;
        Ok(())
    }
}

fn members(result: Result<(), Errno>) -> Result<bool, ToolError> {
    match result {
        Ok(()) => Ok(true),
        Err(error) if no_signalable_members(error) => Ok(false),
        Err(error) => Err(ToolError::Io(format!("group status failed: {error}"))),
    }
}

fn signal_result(result: Result<(), Errno>) -> Result<(), ToolError> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if no_signalable_members(error) => Ok(()),
        Err(error) => Err(ToolError::Io(format!("group signal failed: {error}"))),
    }
}

fn no_signalable_members(error: Errno) -> bool {
    // macOS can return EPERM for our group while its remaining zombies await reaping.
    // Neither that state nor ESRCH leaves a member that can receive a signal.
    matches!(error, Errno::ESRCH | Errno::EPERM)
}
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if !self.cleaned
            && let Err(error) = self.signal(Signal::SIGKILL)
        {
            tracing::warn!(code = error.code(), "process group cleanup failed");
        }
    }
}

#[cfg(test)]
mod tests;
