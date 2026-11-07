//! Results (TK-U-005, TEST-PLAN §15.1): `run.json` holds the environment once and every attempt is one
//! line of `attempts.jsonl`. A result without a complete environment is refused, and a run directory is
//! never overwritten.
use super::classify::{CheckOutcome, Verdict};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tqa_types::{ConversationId, DecisionChoice, DecisionFallbackReason, Event, ServerFrame};

/// Bumped whenever a field changes meaning, so later tools can read old runs.
pub const SCHEMA_VERSION: u32 = 1;

/// Where and on what a run happened.
#[derive(Debug, Serialize)]
pub struct Environment {
    /// The full commit hash of the tested tree.
    pub commit: String,
    /// Whether the tree had uncommitted changes.
    pub dirty: bool,
    /// Image digests by role; `runtime` and `gateway` are required.
    pub images: BTreeMap<String, String>,
    /// The model route name.
    pub route: String,
    /// Whether the route uses native tool calls.
    pub native_tools: bool,
    /// The text protocol's instruction version, which also names its retry sentence (ADR-0007), so a model-fallback
    /// result says which one it had without a commit lookup.
    pub fallback_prompt_version: u32,
    /// The model identifier the route reports.
    pub model: String,
    /// The host.
    pub host: Host,
    /// The driver's version.
    pub driver_version: String,
    /// Every driver flag, with paths but never secrets.
    pub flags: Vec<String>,
}

/// The machine the stack ran on.
#[derive(Debug, Serialize)]
pub struct Host {
    /// The CPU model.
    pub cpu: String,
    /// Logical cores.
    pub cores: u32,
    /// Physical memory in bytes.
    pub memory_bytes: u64,
    /// The kernel release.
    pub kernel: String,
    /// The container engine version.
    pub docker: String,
    /// The GPU, when the model runs locally.
    pub gpu: Option<String>,
}

/// Why an environment is not a result's environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Incomplete(pub &'static str);

impl Environment {
    /// Checks that every field a reader needs is present and well formed.
    ///
    /// # Errors
    /// [`Incomplete`] naming the first missing or malformed field.
    pub fn validate(&self) -> Result<(), Incomplete> {
        let hex = |text: &str, length| {
            text.len() == length
                && text
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        };
        let digest = |text: &String| {
            text.strip_prefix("sha256:")
                .is_some_and(|rest| hex(rest, 64))
        };
        let required = [
            (hex(&self.commit, 40), "commit"),
            (
                ["runtime", "gateway"]
                    .iter()
                    .all(|role| self.images.contains_key(*role)),
                "images",
            ),
            (self.images.values().all(digest), "images"),
            (!self.route.is_empty(), "route"),
            (self.fallback_prompt_version > 0, "fallback_prompt_version"),
            (!self.model.is_empty(), "model"),
            (!self.driver_version.is_empty(), "driver_version"),
            (!self.host.cpu.is_empty() && self.host.cores > 0, "host.cpu"),
            (self.host.memory_bytes > 0, "host.memory_bytes"),
            (!self.host.kernel.is_empty(), "host.kernel"),
            (!self.host.docker.is_empty(), "host.docker"),
        ];
        required
            .into_iter()
            .find_map(|(valid, field)| (!valid).then_some(Incomplete(field)))
            .map_or(Ok(()), Err)
    }
}

/// Counts taken from one attempt's event log.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Tally {
    /// Completed steps.
    pub steps: u32,
    /// Prompt tokens charged.
    pub prompt_tokens: u64,
    /// Completion tokens charged.
    pub completion_tokens: u64,
    /// Compactions recorded.
    pub compactions: u32,
    /// Provider retries recorded.
    pub provider_retries: u32,
}
impl Tally {
    /// Counts the events of `frames`.
    #[must_use]
    pub fn from_events(frames: &[ServerFrame]) -> Self {
        let mut tally = Self::default();
        for frame in frames {
            let ServerFrame::Event { event, .. } = frame else {
                continue;
            };
            match event {
                Event::StepCompleted { .. } => tally.steps += 1,
                Event::Compaction { .. } => tally.compactions += 1,
                Event::ProviderRetry { .. } => tally.provider_retries += 1,
                Event::Usage {
                    prompt_tokens,
                    completion_tokens,
                    ..
                } => {
                    tally.prompt_tokens = tally.prompt_tokens.saturating_add(*prompt_tokens);
                    tally.completion_tokens =
                        tally.completion_tokens.saturating_add(*completion_tokens);
                }
                _ => {}
            }
        }
        tally
    }
}

/// The arm of E2E-DECISION-008 an attempt belongs to: the model deciding alone (the control), or the engine.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arm {
    /// No engine: the model decides (the conversation is made without a profile).
    #[default]
    Model,
    /// The engine decides (the conversation is made from `--engine-profile`).
    Engine,
}

/// An attempt's decisions from its event log (E2E-DECISION-008): each applied decision by kind, each fallback by its
/// reason, the engine's calls and its latency.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DecisionTally {
    /// Applied decisions by kind: `call_model`, `act`, `compact`, `stop` or `select`.
    pub by_kind: BTreeMap<String, u32>,
    /// Fallbacks by reason, as the event names it.
    pub fallbacks: BTreeMap<String, u32>,
    /// Consultations that reached the engine: the decisions and every fallback except `circuit_open` and
    /// `budget_spent`, where nothing was sent.
    pub engine_calls: u64,
    /// The decisions' latency, summed.
    pub engine_latency_ms: u64,
}
impl DecisionTally {
    /// Counts the decision events of `frames`.
    #[must_use]
    pub fn from_events(frames: &[ServerFrame]) -> Self {
        let mut tally = Self::default();
        for frame in frames {
            if let ServerFrame::Event { event, .. } = frame {
                tally.count(event);
            }
        }
        tally
    }

    fn count(&mut self, event: &Event) {
        match event {
            Event::Decision {
                choice, latency_ms, ..
            } => {
                *self.by_kind.entry(kind(choice).to_owned()).or_default() += 1;
                self.engine_calls += 1;
                self.engine_latency_ms = self.engine_latency_ms.saturating_add(*latency_ms);
            }
            Event::DecisionFallback { reason, .. } => {
                if reached_engine(*reason) {
                    self.engine_calls += 1;
                }
                let name = serde_json::to_value(reason)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default();
                *self.fallbacks.entry(name).or_default() += 1;
            }
            _ => {}
        }
    }
}

/// A decision's kind as the tally names it.
const fn kind(choice: &DecisionChoice) -> &'static str {
    match choice {
        DecisionChoice::CallModel { .. } => "call_model",
        DecisionChoice::Act { .. } => "act",
        DecisionChoice::Compact => "compact",
        DecisionChoice::Stop { .. } => "stop",
        DecisionChoice::Select { .. } => "select",
        // A kind added later is still counted, not dropped.
        _ => "other",
    }
}

/// Whether a fallback followed a call to the engine: every reason but an open circuit and a spent budget, where
/// nothing was sent.
const fn reached_engine(reason: DecisionFallbackReason) -> bool {
    !matches!(
        reason,
        DecisionFallbackReason::CircuitOpen | DecisionFallbackReason::BudgetSpent
    )
}

/// One attempt of one task.
#[derive(Debug, Serialize)]
pub struct Attempt {
    /// The task.
    pub task_id: String,
    /// 1-based attempt number within the run.
    pub attempt: u32,
    /// The attempt this one reruns after a provider failure (design 085 D2).
    pub rerun_of: Option<u32>,
    /// The conversation, when one was created.
    pub conversation_id: Option<ConversationId>,
    /// Wall time from sending the task to the verdict.
    pub wall_ms: u64,
    /// Counts from the event log.
    #[serde(flatten)]
    pub tally: Tally,
    /// The check verdict, when the check ran.
    pub check: Option<CheckOutcome>,
    /// Why the driver failed, naming the step, when it did.
    pub detail: Option<String>,
    /// The class and its evidence.
    #[serde(flatten)]
    pub verdict: Verdict,
    /// The attempt's workspace on the server, so one the cleanup left can be found (TK-U-027).
    pub workspace_id: Option<String>,
    /// Its arm (E2E-DECISION-008); `model` when the run compares none.
    pub arm: Arm,
    /// Its round, from 1: with its task, it names the pair the two arms' attempts form (a rerun keeps its round).
    pub round: u32,
    /// Its place in its round's pair, 1 or 2: the order alternates per round (ABBA), so neither arm always runs on a
    /// cache the other warmed. 1 when the run compares none.
    pub pair_position: u32,
    /// Its decisions, from the event log.
    pub decisions: DecisionTally,
}

/// Writes one run directory.
#[derive(Debug)]
pub struct ResultWriter {
    directory: PathBuf,
}
impl ResultWriter {
    /// Creates `directory` and writes `run.json`; an existing run is never overwritten.
    ///
    /// # Errors
    /// An incomplete environment or an I/O failure, including an existing `run.json`.
    pub fn create(directory: &Path, environment: &Environment) -> std::io::Result<Self> {
        environment.validate().map_err(|Incomplete(field)| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, field)
        })?;
        fs::create_dir_all(directory)?;
        let header =
            serde_json::json!({ "schema_version": SCHEMA_VERSION, "environment": environment });
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("run.json"))?;
        serde_json::to_writer_pretty(&mut file, &header)?;
        Ok(Self {
            directory: directory.to_owned(),
        })
    }
    /// Appends one attempt as a JSON line.
    ///
    /// # Errors
    /// An I/O failure.
    pub fn append(&self, attempt: &Attempt) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(attempt)?;
        line.push(b'\n');
        let path = self.directory.join("attempts.jsonl");
        fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?
            .write_all(&line)
    }
}

#[cfg(test)]
mod decision_tests;
#[cfg(test)]
mod tests;
