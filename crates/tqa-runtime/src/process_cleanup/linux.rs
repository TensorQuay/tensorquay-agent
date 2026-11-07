//! Specific-PID reaping never consumes a child owned by the asynchronous process driver.
use super::SupervisedGroups;
use crate::{RuntimeError, RuntimeLimits};
use rustix::{
    io::Errno,
    process::{self, Pid, Signal, WaitOptions},
};
use std::{collections::HashMap, io::Read as _, sync::Arc, time::Duration};
use tqa_types::{Clock, Sleeper};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const STAT_BYTES: u64 = 4096;
/// In a desktop session, how long an orphan has to become the display's client before it is reaped (ADR-0039 section
/// 10): an application a call starts in the background may connect only after the call has ended.
pub(super) const GRACE: Duration = Duration::from_secs(10);

/// A direct child, and when it started (`/proc/<pid>/stat`'s starttime, in clock ticks since boot): a pid looked at
/// again later is the same process only if it started at the same time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Child {
    pub(super) pid: Pid,
    pub(super) start: u64,
}

/// One pass: whether nothing was left to kill, and the young orphans it left to decide later.
struct Pass {
    done: bool,
    deferred: Vec<Child>,
}
pub(super) struct Reaper {
    clock: Arc<dyn Clock>,
    sleeper: Arc<dyn Sleeper>,
    limits: RuntimeLimits,
    pub(super) supervised: SupervisedGroups,
}
impl Reaper {
    pub fn new(
        clock: Arc<dyn Clock>,
        sleeper: Arc<dyn Sleeper>,
        limits: RuntimeLimits,
    ) -> Result<Self, RuntimeError> {
        process::set_child_subreaper(Some(Pid::INIT))
            .map_err(|_| RuntimeError::io("child subreaper"))?;
        Ok(Self {
            clock,
            sleeper,
            limits,
            supervised: SupervisedGroups::default(),
        })
    }
    pub async fn reap(&self) -> Result<(), RuntimeError> {
        let started = self.clock.now();
        let mut deferred: Vec<Child> = Vec::new();
        loop {
            let limit = self.limits.max_children;
            let supervised = self.supervised.clone();
            let mut pass = tokio::task::spawn_blocking(move || reap_pass(limit, &supervised));
            let remaining = self
                .limits
                .reap_timeout
                .saturating_sub(self.clock.now().saturating_duration_since(started));
            let result = tokio::select! {biased;
                result=&mut pass=>result.map_err(|_|RuntimeError::Worker)?,
                ()=self.sleeper.sleep(remaining)=>{pass.abort();return Err(RuntimeError::Timeout);}
            }?;
            let fresh: Vec<Child> = result
                .deferred
                .into_iter()
                .filter(|child| !deferred.contains(child))
                .collect();
            deferred.extend(fresh);
            if result.done {
                self.decide_later(deferred);
                return Ok(());
            }
            if self.clock.now().saturating_duration_since(started) >= self.limits.reap_timeout {
                return Err(RuntimeError::Timeout);
            }
            self.sleeper.sleep(POLL_INTERVAL.min(remaining)).await;
        }
    }
}
impl Reaper {
    /// Once [`GRACE`] has passed, the young orphans a reap left: each still the runtime's child (the same process, by
    /// its start) and still not the display's client is killed; one that became a client stays. Only these pids are
    /// looked at, never every child, so a shell call in flight meanwhile is untouched.
    fn decide_later(&self, deferred: Vec<Child>) {
        if deferred.is_empty() {
            return;
        }
        let sleeper = self.sleeper.clone();
        let supervised = self.supervised.clone();
        let limit = self.limits.max_children;
        tokio::spawn(async move {
            sleeper.sleep(GRACE).await;
            let decided =
                tokio::task::spawn_blocking(move || decide(&deferred, &supervised, limit)).await;
            if !matches!(decided, Ok(Ok(()))) {
                tracing::warn!("the young orphans' later reap failed");
            }
        });
    }
}

fn reap_pass(limit: usize, supervised: &SupervisedGroups) -> Result<Pass, RuntimeError> {
    // A recorded MCP server's or the browser's leader and its group are left alone, and in a desktop session so are
    // the display's clients (an application the agent opened) and, until a later decision, orphans too young to have
    // become one; every other direct child is killed with its group.
    let mut children: Vec<Child> = children(limit)?
        .into_iter()
        .filter(|child| !supervised.contains(child.pid.as_raw_nonzero().get()))
        .collect();
    let mut deferred = Vec::new();
    if let Some(display) = supervised.display()
        && !children.is_empty()
    {
        let clients = super::display::clients(&display, limit);
        children.retain(|child| !super::display::holds(child.pid.as_raw_nonzero().get(), &clients));
        let now = uptime()?;
        (deferred, children) = children
            .into_iter()
            .partition(|child| young(child.start, now));
    }
    for child in &children {
        kill(child.pid)?;
    }
    Ok(Pass {
        done: children.is_empty(),
        deferred,
    })
}

/// The deferred orphans that are still the runtime's children, the same processes, and still not the display's
/// clients, are killed.
fn decide(
    deferred: &[Child],
    supervised: &SupervisedGroups,
    limit: usize,
) -> Result<(), RuntimeError> {
    let now: HashMap<Pid, u64> = children(limit)?
        .into_iter()
        .map(|child| (child.pid, child.start))
        .collect();
    let clients = supervised
        .display()
        .map(|display| super::display::clients(&display, limit))
        .unwrap_or_default();
    for child in deferred {
        let same = now.get(&child.pid) == Some(&child.start);
        let raw = child.pid.as_raw_nonzero().get();
        if same && !supervised.contains(raw) && !super::display::holds(raw, &clients) {
            kill(child.pid)?;
        }
    }
    Ok(())
}

/// Kills a child with its group (when it has one of its own) and reaps it.
fn kill(pid: Pid) -> Result<(), RuntimeError> {
    match process::getpgid(Some(pid)) {
        Ok(group) if group != process::getpgrp() => {
            ignore_gone(process::kill_process_group(group, Signal::KILL))?;
        }
        Ok(_) => {}
        Err(Errno::SRCH) => return Ok(()),
        Err(_) => return Err(RuntimeError::io("process group lookup")),
    }
    ignore_gone(process::kill_process(pid, Signal::KILL))?;
    match process::waitpid(Some(pid), WaitOptions::NOHANG) {
        Ok(_) | Err(Errno::CHILD) => Ok(()),
        Err(_) => Err(RuntimeError::io("process reap")),
    }
}

/// Whether a process that started at `start` is younger than [`GRACE`] at `now` (both in clock ticks since boot).
pub(super) fn young(start: u64, now: u64) -> bool {
    let ticks = rustix::param::clock_ticks_per_second();
    now.saturating_sub(start) < GRACE.as_secs().saturating_mul(ticks)
}

/// Clock ticks since boot, from `/proc/uptime`.
fn uptime() -> Result<u64, RuntimeError> {
    let text =
        std::fs::read_to_string("/proc/uptime").map_err(|_| RuntimeError::io("uptime read"))?;
    uptime_ticks(&text, rustix::param::clock_ticks_per_second())
        .ok_or(RuntimeError::io("uptime parse"))
}

/// `/proc/uptime`'s first field (`<seconds>.<hundredths>`) in clock ticks, without floating point.
pub(super) fn uptime_ticks(text: &str, ticks: u64) -> Option<u64> {
    let (seconds, hundredths) = text.split_whitespace().next()?.split_once('.')?;
    let seconds: u64 = seconds.parse().ok()?;
    let hundredths: u64 = hundredths.get(..2)?.parse().ok()?;
    Some(seconds.checked_mul(ticks)? + hundredths * ticks / 100)
}
fn ignore_gone(result: Result<(), Errno>) -> Result<(), RuntimeError> {
    match result {
        Ok(()) | Err(Errno::SRCH) => Ok(()),
        Err(_) => Err(RuntimeError::io("process signal")),
    }
}
fn children(limit: usize) -> Result<Vec<Child>, RuntimeError> {
    let entries = std::fs::read_dir("/proc").map_err(|_| RuntimeError::io("proc listing"))?;
    let own = process::getpid().as_raw_nonzero().get();
    let mut inspected = 0;
    let mut children = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| RuntimeError::io("proc listing"))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|v| v.parse().ok())
            .and_then(Pid::from_raw)
        else {
            continue;
        };
        inspected += 1;
        if inspected > limit {
            return Err(RuntimeError::Limit);
        }
        let file = match std::fs::File::open(entry.path().join("stat")) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(RuntimeError::io("proc stat open")),
        };
        let mut bytes = Vec::new();
        file.take(STAT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| RuntimeError::io("proc stat read"))?;
        if bytes.len() as u64 > STAT_BYTES {
            return Err(RuntimeError::Limit);
        }
        let line = std::str::from_utf8(&bytes).map_err(|_| RuntimeError::io("proc stat parse"))?;
        let (parent, start) = parent_and_start(line).ok_or(RuntimeError::io("proc stat parse"))?;
        if parent == own {
            children.push(Child { pid, start });
        }
    }
    Ok(children)
}

/// `/proc/<pid>/stat`'s parent pid and starttime: the 4th and 22nd fields, counted after the command's closing
/// parenthesis since a command may hold spaces and parentheses.
pub(super) fn parent_and_start(stat: &str) -> Option<(i32, u64)> {
    let (_, fields) = stat.rsplit_once(") ")?;
    let fields: Vec<&str> = fields.split_whitespace().collect();
    Some((fields.get(1)?.parse().ok()?, fields.get(19)?.parse().ok()?))
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod tests;
