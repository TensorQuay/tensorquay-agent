//! One attempt through the public API (TEST-PLAN §16): create the conversation on the seeded workspace,
//! send the task, follow the stream while enforcing the task budgets,
//! then replay the authoritative log for the integrity check.
use super::{integrity::seq, manifest::Manifest, sse::SseDecoder};
use crate::client::{ApiClient, ClientError, NewConversation};
use std::time::{Duration, Instant};
use tqa_types::{ConversationId, Event, RunId, Seq, ServerFrame};

/// A silent stream is reopened after this long; the gateway's keep-alives arrive far more often.
const STREAM_IDLE: Duration = Duration::from_secs(90);
/// Stream reopenings per attempt before the driver gives up.
const REOPENINGS: u32 = 3;
/// Frames per replay page; the gateway's maximum.
const PAGE: u32 = 1000;
/// Replay pages per attempt, bounding a runaway log at a million frames.
const MAX_PAGES: usize = 1000;

/// What to run.
#[derive(Debug)]
pub struct Plan<'a> {
    /// The task.
    pub manifest: &'a Manifest,
    /// The message sent to the agent: the task's `task.md`.
    pub task: &'a str,
    /// Further messages, each sent once the previous run completed (a multi-turn task such as the compaction demo).
    pub followups: &'a [String],
    /// The model route.
    pub route: &'a str,
    /// The environment template, or the gateway default.
    pub template: Option<&'a str>,
    /// The profile to make the conversation from (the engine arm of E2E-DECISION-008), or none.
    pub profile: Option<&'a str>,
    /// The seeded workspace (or its fork) the conversation binds to.
    pub workspace_id: &'a str,
    /// Time past the task's wall clock before the driver stops waiting (60 s in the driver).
    pub margin: Duration,
}

/// What the driver observed; the workspace copy and the check follow separately.
#[derive(Debug, Default)]
pub struct Followed {
    /// The conversation, once created.
    pub conversation_id: Option<ConversationId>,
    /// Frames as the stream delivered them.
    pub streamed: Vec<ServerFrame>,
    /// The authoritative log after the run.
    pub replayed: Vec<ServerFrame>,
    /// The driver cancelled the run for exceeding a task budget.
    pub over_budget: bool,
    /// The deadline passed before a terminal event.
    pub deadline_passed: bool,
    /// The API failed, or the stream broke beyond recovery.
    pub driver_failed: bool,
    /// The failing step and error when the driver failed, so a harness failure is never unexplained.
    pub driver_detail: Option<String>,
    /// From sending the task to the end of the replay.
    pub wall: Duration,
}

#[derive(Default)]
struct Progress {
    run: Option<RunId>,
    /// The latest terminal event was `run.completed`.
    completed: bool,
    last: Seq,
    steps: u32,
    tokens: u64,
}

/// Runs one attempt; every failure is recorded in [`Followed`] for classification, never returned.
pub async fn follow(client: &ApiClient, plan: &Plan<'_>) -> Followed {
    let started = Instant::now();
    let mut followed = Followed::default();
    let mut progress = Progress::default();
    let wall_clock = Duration::from_secs(plan.manifest.budgets.wall_clock_secs.into());
    let deadline = wall_clock.saturating_add(plan.margin);
    match tokio::time::timeout(deadline, drive(client, plan, &mut followed, &mut progress)).await {
        Ok(Ok(())) => {}
        Ok(Err((step, error))) => followed.fail(step, error),
        Err(_) => followed.deadline_passed = true,
    }
    if let Some(id) = followed.conversation_id.clone() {
        if followed.deadline_passed
            && let Some(run) = &progress.run
            && let Err(error) = client.cancel(&id, run).await
        {
            followed.fail("cancel after the deadline", error);
        }
        match replay(client, &id).await {
            Ok(frames) => followed.replayed = frames,
            Err(error) => followed.fail("replay the event log", error),
        }
    }
    followed.wall = started.elapsed();
    followed
}

/// A failed step: what the driver was doing, and the error.
type Fault = (&'static str, ClientError);

impl Followed {
    fn fail(&mut self, step: &str, error: impl std::fmt::Display) {
        self.driver_failed = true;
        self.driver_detail
            .get_or_insert_with(|| format!("{step}: {error}"));
    }
}

async fn drive(
    client: &ApiClient,
    plan: &Plan<'_>,
    followed: &mut Followed,
    progress: &mut Progress,
) -> Result<(), Fault> {
    let request = NewConversation {
        title: &plan.manifest.id,
        model_route: plan.route,
        template: plan.template,
        profile: plan.profile,
        workspace_id: Some(plan.workspace_id),
    };
    let at = |step| move |error| (step, error);
    let id = client
        .create_conversation(&request)
        .await
        .map_err(at("create the conversation"))?;
    followed.conversation_id = Some(id.clone());
    client
        .send_message(&id, plan.task)
        .await
        .map_err(at("send the task"))?;
    let lifetime = Duration::from_secs(plan.manifest.budgets.wall_clock_secs.into())
        .saturating_add(plan.margin);
    let session = Session { client, plan, id };
    let (mut followups, mut breaks) = (plan.followups.iter(), 0);
    loop {
        let response = client
            .stream(&session.id, progress.last, lifetime)
            .await
            .map_err(at("open the event stream"))?;
        let ended = session
            .read(response, followed, progress)
            .await
            .map_err(at("follow the event stream"))?;
        if !ended {
            breaks += 1;
            if breaks > REOPENINGS {
                return Err(("follow the event stream", ClientError::Transport));
            }
            continue;
        }
        // The next message goes out only after a completed run; any other end finishes the attempt.
        match followups.next().filter(|_| progress.completed) {
            Some(text) => {
                client
                    .send_message(&session.id, text)
                    .await
                    .map_err(at("send a follow-up message"))?;
                progress.run = None;
            }
            None => return Ok(()),
        }
    }
}

/// One conversation of one attempt.
struct Session<'a, 'p> {
    client: &'a ApiClient,
    plan: &'a Plan<'p>,
    id: ConversationId,
}
impl Session<'_, '_> {
    /// Reads one stream response until the run ends (true), or the stream breaks or goes silent (false).
    async fn read(
        &self,
        mut response: reqwest::Response,
        followed: &mut Followed,
        progress: &mut Progress,
    ) -> Result<bool, ClientError> {
        let mut decoder = SseDecoder::default();
        while let Ok(Ok(Some(chunk))) = tokio::time::timeout(STREAM_IDLE, response.chunk()).await {
            let frames = decoder.push(&chunk).map_err(|_| ClientError::Decode)?;
            if self.take(frames, followed, progress).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Observes and records frames in order; returns whether one ended the run.
    async fn take(
        &self,
        frames: Vec<ServerFrame>,
        followed: &mut Followed,
        progress: &mut Progress,
    ) -> Result<bool, ClientError> {
        for frame in frames {
            let done = self.observe(&frame, followed, progress).await?;
            followed.streamed.push(frame);
            if done {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Acts on one frame; returns whether it ended the run.
    async fn observe(
        &self,
        frame: &ServerFrame,
        followed: &mut Followed,
        progress: &mut Progress,
    ) -> Result<bool, ClientError> {
        if let Some(number) = seq(frame) {
            progress.last = number;
        }
        match frame {
            // Before a run starts, an error frame means the gateway did not deliver the task: no run will
            // follow, so the attempt ends now instead of at its deadline.
            ServerFrame::Error { code, message, .. } if progress.run.is_none() => {
                let code = serde_json::to_value(code)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned));
                followed.fail(
                    "deliver the task",
                    format!("{}: {message}", code.unwrap_or_default()),
                );
                Ok(true)
            }
            ServerFrame::Event { event, .. } => {
                if progress.count(event) {
                    return Ok(true);
                }
                let budgets = &self.plan.manifest.budgets;
                let exceeded =
                    progress.steps > budgets.max_steps || progress.tokens > budgets.max_tokens;
                if exceeded
                    && !followed.over_budget
                    && let Some(run) = &progress.run
                {
                    self.client.cancel(&self.id, run).await?;
                    followed.over_budget = true;
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }
}

impl Progress {
    /// Counts one event; returns whether it is the run's terminal event.
    fn count(&mut self, event: &Event) -> bool {
        match event {
            Event::RunStarted { run_id } => self.run = Some(run_id.clone()),
            Event::StepCompleted { .. } => self.steps += 1,
            Event::Usage {
                prompt_tokens,
                completion_tokens,
                ..
            } => {
                self.tokens = self
                    .tokens
                    .saturating_add(prompt_tokens.saturating_add(*completion_tokens));
            }
            Event::RunCompleted { .. } | Event::RunFailed { .. } | Event::RunCancelled { .. } => {
                self.completed = matches!(event, Event::RunCompleted { .. });
                return true;
            }
            _ => {}
        }
        false
    }
}

async fn replay(client: &ApiClient, id: &ConversationId) -> Result<Vec<ServerFrame>, ClientError> {
    let mut frames = Vec::new();
    let mut after = Seq::ZERO;
    for _ in 0..MAX_PAGES {
        let page = client.events(id, after, PAGE).await?;
        let Some(next) = page.next_after else {
            return Ok(frames);
        };
        let full = page.items.len() == PAGE as usize;
        frames.extend(page.items);
        if !full {
            return Ok(frames);
        }
        after = next;
    }
    Err(ClientError::TooLarge)
}

#[cfg(test)]
mod tests;
