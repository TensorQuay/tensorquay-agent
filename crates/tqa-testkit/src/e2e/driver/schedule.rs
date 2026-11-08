//! `--concurrency N` scheduling: which attempt starts next, and its number. Pure, so the rules are tested
//! without a stack (TK-U-024).
//!
//! - At most N attempts run at once, and they start in task and round order.
//! - An attempt is numbered when it starts. A `harness.provider` rerun goes first in the queue, so at N=1 it's numbered
//!   right after the attempt it repeats, exactly as the one-at-a-time driver did.
//! - With N>1, a harness failure (other than a first `harness.provider`, which gets its rerun) stops new starts; the
//!   running attempts finish and the rest are recorded as skipped. At N=1 the run goes on, as before.
//! - The token limit stops new starts once the attempts finished so far reach it.
//! - With arms (E2E-DECISION-008, TK-U-033), each task runs its rounds once per arm, interleaved per task and round,
//!   the pair's order alternating per round (ABBA); a rerun keeps its arm and place. The engine-call cap and the wall-time cap stop new starts for both arms alike.
use crate::e2e::{classify::Class, result::Arm};
use std::{collections::VecDeque, time::Duration};

/// One attempt to run: a task (by index), its round, and the attempt it reruns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Job {
    /// The task's index in the task set.
    pub task: usize,
    /// The round, from 1.
    pub round: u32,
    /// The attempt this one reruns, for a `harness.provider` rerun.
    pub rerun_of: Option<u32>,
    /// Its arm: the model alone, or the engine.
    pub arm: Arm,
    /// Its place in its round's pair, from 1.
    pub position: u32,
}

/// An attempt that may start now, with its number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Started {
    /// The attempt's number, from 1.
    pub number: u32,
    /// What it runs.
    pub job: Job,
}

/// Why an attempt never started.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Skip {
    /// The run's token limit was reached.
    Budget,
    /// New starts stopped after a harness failure in attempt `by` (N>1 only).
    Paused {
        /// The attempt whose harness failure stopped new starts.
        by: u32,
    },
    /// The run's engine calls reached `--max-engine-calls`.
    EngineBudget,
    /// The run's time reached `--max-wall-minutes`.
    WallTime,
}

/// The run's queue of attempts and the rules for starting them.
pub(super) struct Schedule {
    queue: VecDeque<Job>,
    tasks: usize,
    repeat: u32,
    engine_limit: Option<u64>,
    engine_calls: u64,
    wall_limit: Option<Duration>,
    concurrency: usize,
    limit: Option<u64>,
    running: usize,
    started: u32,
    spent: u64,
    paused_by: Option<u32>,
    skipped: Vec<(Job, Skip)>,
}

impl Schedule {
    /// Every task × round, in order, to run at most `concurrency` at once (at least 1).
    pub fn new(tasks: usize, repeat: u32, concurrency: usize, limit: Option<u64>) -> Self {
        Self {
            queue: jobs(tasks, repeat, &[Arm::Model]),
            tasks,
            repeat,
            engine_limit: None,
            engine_calls: 0,
            wall_limit: None,
            concurrency: concurrency.max(1),
            limit,
            running: 0,
            started: 0,
            spent: 0,
            paused_by: None,
            skipped: Vec::new(),
        }
    }

    /// Every task × round × arm, interleaved per task and round (the first arm first); built before any start.
    pub fn with_arms(mut self, arms: &[Arm]) -> Self {
        self.queue = jobs(self.tasks, self.repeat, arms);
        self
    }

    /// Stops new starts once the run's engine calls reach `limit`.
    pub const fn engine_limit(mut self, limit: Option<u64>) -> Self {
        self.engine_limit = limit;
        self
    }

    /// Stops new starts once the run has lasted `limit`.
    pub const fn wall_limit(mut self, limit: Option<Duration>) -> Self {
        self.wall_limit = limit;
        self
    }

    /// Adds a finished attempt's engine calls to the run's.
    pub const fn record_engine_calls(&mut self, calls: u64) {
        self.engine_calls = self.engine_calls.saturating_add(calls);
    }

    /// The next attempt to start now, numbered, or `None` when none may start (all slots busy, the queue empty,
    /// stopped by the token limit or by a pause).
    #[cfg(test)]
    pub fn start(&mut self) -> Option<Started> {
        self.start_at(Duration::ZERO)
    }

    /// As [`Self::start`], `elapsed` into the run, for the wall-time cap.
    pub fn start_at(&mut self, elapsed: Duration) -> Option<Started> {
        loop {
            if self.running >= self.concurrency {
                return None;
            }
            let job = self.queue.pop_front()?;
            if let Some(by) = self.paused_by {
                self.skipped.push((job, Skip::Paused { by }));
                continue;
            }
            if self.limit.is_some_and(|limit| self.spent >= limit) {
                self.skipped.push((job, Skip::Budget));
                continue;
            }
            if self
                .engine_limit
                .is_some_and(|limit| self.engine_calls >= limit)
            {
                self.skipped.push((job, Skip::EngineBudget));
                continue;
            }
            if self.wall_limit.is_some_and(|limit| elapsed >= limit) {
                self.skipped.push((job, Skip::WallTime));
                continue;
            }
            self.running += 1;
            self.started += 1;
            return Some(Started {
                number: self.started,
                job,
            });
        }
    }

    /// Records a finished attempt: its tokens, its class, and a rerun or a pause when its class calls for one.
    pub fn finish(&mut self, number: u32, job: &Job, class: Class, tokens: u64) {
        self.running = self.running.saturating_sub(1);
        self.spent = self.spent.saturating_add(tokens);
        let first_provider = class == Class::HarnessProvider && job.rerun_of.is_none();
        if first_provider {
            self.queue.push_front(Job {
                rerun_of: Some(number),
                ..*job
            });
        } else if class.is_harness() && self.concurrency > 1 && self.paused_by.is_none() {
            self.paused_by = Some(number);
        }
    }

    /// Whether nothing is queued and nothing runs.
    pub fn is_done(&self) -> bool {
        self.running == 0 && self.queue.is_empty()
    }

    /// The attempt whose harness failure stopped new starts, if one did.
    #[cfg(test)]
    pub fn paused_by(&self) -> Option<u32> {
        self.paused_by
    }

    /// The attempts that never started, and why, in queue order.
    pub fn skipped(&self) -> &[(Job, Skip)] {
        &self.skipped
    }
}

/// Every task × round × arm in start order: per task, per round, each arm in turn, the order reversed every other
/// round (ABBA), so neither arm always runs second, on a cache the other warmed.
fn jobs(tasks: usize, repeat: u32, arms: &[Arm]) -> VecDeque<Job> {
    let arms = if arms.is_empty() {
        &[Arm::Model][..]
    } else {
        arms
    };
    let mut queue = VecDeque::new();
    for task in 0..tasks {
        for round in 1..=repeat {
            let mut order = arms.to_vec();
            if round % 2 == 0 {
                order.reverse();
            }
            for (index, arm) in order.into_iter().enumerate() {
                queue.push_back(Job {
                    task,
                    round,
                    rerun_of: None,
                    arm,
                    position: u32::try_from(index + 1).unwrap_or(u32::MAX),
                });
            }
        }
    }
    queue
}
