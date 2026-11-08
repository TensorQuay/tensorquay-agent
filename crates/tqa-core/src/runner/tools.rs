//! Ordered batches preserve read/write dependencies while overlapping consecutive safe calls.
use super::{
    journal,
    session::{Execution, Stop},
};
use crate::{AFTER_WAIT, BEFORE_WAIT, CoreError, limits::SPACE_QUERY_TIMEOUT};
use serde_json::Value;
use std::collections::BTreeSet;
use tqa_tools::{ToolContext, ToolError, ToolOutput};
use tqa_types::{
    CapturePhase, ErrorCode, Event, ScreenEvidence, Sleeper, ToolStatus, chunk_tool_output,
};

impl Execution<'_> {
    pub(super) async fn tool_phase(&mut self, selected: &BTreeSet<String>) -> Result<(), Stop> {
        let calls: Vec<_> = self
            .current_attempt()?
            .calls
            .into_iter()
            .filter(|call| call.result.is_none())
            .collect();
        let registry = self.runner.tools.clone();
        let parallel = |call: &journal::Call| {
            registry
                .get(&call.name)
                .is_some_and(|tool| tool.spec().parallel_safe)
        };
        for batch in calls.chunk_by(|left, right| parallel(left) && parallel(right)) {
            for call in batch {
                self.dispatch(call, selected.contains(call.id.as_str()))
                    .await?;
            }
            while !self.jobs.is_empty() {
                self.finish_next_tool().await?;
            }
        }
        Ok(())
    }
    pub(super) async fn finish_next_tool(&mut self) -> Result<(), Stop> {
        let finished = self
            .watch()
            .wait(self.jobs.join_next())
            .await?
            .ok_or(CoreError::Internal("missing owned tool worker"))?
            .map_err(|_| CoreError::Internal("tool worker failed"))?;
        self.tool_finished(finished.0, finished.1, finished.2).await
    }
    async fn reserve_failure_slot(&mut self, call: &journal::Call) -> Result<(), Stop> {
        loop {
            let registry = &self.runner.tools;
            let changes = |name: &str| {
                registry
                    .get(name)
                    .is_some_and(|tool| tool.spec().changes_workspace)
            };
            let failed = journal::failure_count(&self.frames, &self.run_id()?, call, changes);
            let pending = self
                .current_attempt()?
                .calls
                .iter()
                .filter(|other| {
                    other.started
                        && other.result.is_none()
                        && other.name == call.name
                        && other.arguments == call.arguments
                })
                .count();
            let pending = u32::try_from(pending).unwrap_or(u32::MAX);
            // A copy launches only while neither limit could be reached by those already running (CORE-E-041).
            if failed.recent.saturating_add(pending) < journal::REPEATED_FAILURES
                && failed.total.saturating_add(pending) < journal::RUN_FAILURES
            {
                return Ok(());
            }
            self.finish_next_tool().await?;
        }
    }
    async fn dispatch(&mut self, call: &journal::Call, selected: bool) -> Result<(), Stop> {
        self.guard()?;
        if !selected {
            return self
                .result(
                    call,
                    ToolStatus::Error,
                    "not run: skipped by the decision engine".into(),
                    Beside::default(),
                )
                .await;
        }
        let arguments = match object_arguments(&call.arguments) {
            Ok(arguments) => arguments,
            Err(error) => {
                return self
                    .result(
                        call,
                        ToolStatus::Error,
                        error.model_message(),
                        Beside::default(),
                    )
                    .await;
            }
        };
        let spec = match self.runner.tools.prepare(&call.name, &arguments) {
            Ok(spec) => spec.clone(),
            Err(error) => {
                return self
                    .result(
                        call,
                        ToolStatus::Error,
                        error.model_message(),
                        Beside::default(),
                    )
                    .await;
            }
        };
        let Some(action) = self.prepare(call, arguments, &spec).await? else {
            return Ok(());
        };
        self.reserve_failure_slot(call).await?;
        let cx = ToolContext {
            workspace: self.runner.workspace.clone(),
            cancel: self.children.child_token(),
            clock: self.runner.clock.clone(),
            sleeper: self.runner.sleeper.clone(),
            env_allow: self.runner.env_allow.clone(),
            limits: self.runner.tool_limits,
        };
        // The screen before an action, journaled ahead of its start (ADR-0039 "The record"); kept or not, the action
        // goes on. Taken only for the desktop's own tool, and when there is a store to keep it in.
        let before = if self.runner.screen_store.is_some() && call.name == "computer" {
            self.watch()
                .wait(self.runner.tools.before(&action, &cx))
                .await?
        } else {
            None
        };
        let before_kept = match before {
            Some(image) => {
                self.capture(&call.id, CapturePhase::Before, &image, BEFORE_WAIT)
                    .await?
            }
            None => false,
        };
        self.emit(Event::ToolStart {
            call_id: call.id.clone(),
        })
        .await?;
        let tools = self.runner.tools.clone();
        let mut call = call.clone();
        call.started = true;
        self.jobs.spawn(async move {
            let result = tools.call_prepared(action, &cx).await;
            (call, result, before_kept)
        });
        Ok(())
    }
    async fn tool_finished(
        &mut self,
        call: journal::Call,
        result: Result<ToolOutput, ToolError>,
        before_kept: bool,
    ) -> Result<(), Stop> {
        // Out of space: a tool said so, or a command failed while the workspace is out of space (a shell reports
        // ENOSPC only in its own output). The kernel query runs only after a failed command.
        let full = match &result {
            Err(error) => *error == ToolError::WorkspaceFull,
            Ok(output) if output.exit_code.is_some_and(|code| code != 0) => {
                let workspace = self.runner.workspace.clone();
                bounded_query(move || workspace.space_exhausted(), &*self.runner.sleeper).await
            }
            Ok(_) => false,
        };
        // The element a computer action acted on, for its tool.result only: the model reads the content alone.
        let target = result
            .as_ref()
            .ok()
            .and_then(|output| output.target.clone());
        // A screenshot for the model is kept in the run's state, never journaled; a failed write only means the model
        // gets its fixed line.
        if let (Ok(output), Some(screenshots)) = (&result, &self.runner.screenshots)
            && let Some(image) = &output.image
            && let Some(run) = &self.context.run_id
            && let Err(error) = screenshots
                .keep(run, &call.id, &image.png, image.plain.as_deref())
                .await
        {
            tracing::warn!(reason = %error, "a screenshot couldn't be kept for the model");
        }
        let evidence = match &result {
            Ok(output) => self.record_captures(&call, output, before_kept).await?,
            Err(_) => None,
        };
        let (status, text, exit_code, failure) = match result {
            Ok(output) => (
                if output.exit_code.is_some_and(|code| code != 0) {
                    ToolStatus::Error
                } else {
                    ToolStatus::Ok
                },
                match super::captures::not_kept_note(evidence.as_ref()) {
                    Some(note) => format!("{}\n{note}", output.text),
                    None => output.text,
                },
                output.exit_code,
                None,
            ),
            Err(error) => (
                if error == ToolError::Cancelled {
                    ToolStatus::Cancelled
                } else {
                    ToolStatus::Error
                },
                error.model_message(),
                None,
                // Why, as data: a client words the failure itself, never from the model's text (CORE-U-076).
                (error != ToolError::Cancelled)
                    .then(|| {
                        tqa_types::ToolFailure::new(error.failure_code(), error.target().cloned())
                            .ok()
                    })
                    .flatten(),
            ),
        };
        for event in chunk_tool_output(call.id.clone(), &text) {
            self.emit(event).await?;
        }
        self.emit(Event::ToolEnd {
            call_id: call.id.clone(),
            status,
            exit_code,
            failure,
        })
        .await?;
        // Once per run, also across a resume: the journal says whether this run already reported it (SBX-C-015).
        if full && !journal::workspace_full_reported(&self.frames, &self.run_id()?) {
            self.emit(Event::WorkspaceFull {
                call_id: call.id.clone(),
            })
            .await?;
        }
        self.result(&call, status, text, Beside { target, evidence })
            .await
    }
    /// Stores a finished `computer` call's after capture, or its screenshot for the model, ahead of its result; what
    /// its `tool.result` says was kept. An action that reached the screen says both phases, a screenshot its one
    /// capture; any other tool (an image another tool returns is no screenshot of the desktop), or a run without a
    /// store, says nothing.
    async fn record_captures(
        &mut self,
        call: &journal::Call,
        output: &ToolOutput,
        before_kept: bool,
    ) -> Result<Option<ScreenEvidence>, Stop> {
        if self.runner.screen_store.is_none() || call.name != "computer" {
            return Ok(None);
        }
        if let Some(evidence) = &output.evidence {
            let after = match &evidence.after {
                Some(image) => {
                    self.capture(&call.id, CapturePhase::After, image, AFTER_WAIT)
                        .await?
                }
                None => false,
            };
            return Ok(Some(ScreenEvidence {
                before: Some(before_kept),
                after,
            }));
        }
        let Some(image) = &output.image else {
            return Ok(None);
        };
        let after = self
            .capture(&call.id, CapturePhase::Screenshot, image, AFTER_WAIT)
            .await?;
        Ok(Some(ScreenEvidence {
            before: None,
            after,
        }))
    }
    pub(super) async fn result(
        &mut self,
        call: &journal::Call,
        status: ToolStatus,
        text: String,
        beside: Beside,
    ) -> Result<(), Stop> {
        let directory = self.runner.spill_dir.clone();
        let configured = self.runner.limits.tool_result_bytes as usize;
        // A result takes at most a share of the input capacity still free, so it and the next request fit the window
        // (CTX-E-004).
        let (needed, capacity) =
            super::model::request::measure_pending(self, &call.context).await?;
        let limit = tqa_context::window_result_cap(configured, needed, capacity);
        let id = call.id.clone();
        self.spills
            .spawn_blocking(move || tqa_context::cap_tool_result(&id, &text, limit, &directory));
        let capped = self
            .watch()
            .wait(self.spills.join_next())
            .await?
            .ok_or(CoreError::Internal("missing owned spill worker"))?
            .map_err(|_| CoreError::Internal("spill worker failed"))?;
        let (status, content) = match capped {
            Ok(capped) => (status, capped.content),
            Err(_) => (
                ToolStatus::Error,
                ToolError::Io("tool result could not be preserved".into()).model_message(),
            ),
        };
        self.emit(Event::ToolResult {
            call_id: call.id.clone(),
            status,
            content,
            target: beside.target,
            evidence: beside.evidence,
        })
        .await?;
        self.repeated_failure()
    }

    /// Stops the run when one call has failed too often: three times with no change between, or six times in all
    /// (CORE-E-012, CORE-E-041).
    pub(super) fn repeated_failure(&self) -> Result<(), Stop> {
        let registry = &self.runner.tools;
        let changes = |name: &str| {
            registry
                .get(name)
                .is_some_and(|tool| tool.spec().changes_workspace)
        };
        match journal::repeated(&self.frames, &self.run_id()?, changes) {
            Some(reason) => Err(Stop::Failed(ErrorCode::RepeatedFailure, reason.into())),
            None => Ok(()),
        }
    }
}
/// What a `computer` call's `tool.result` carries beside its content, for clients only: never part of the model's input.
#[derive(Default)]
pub(super) struct Beside {
    /// The element an action acted on by ref.
    pub target: Option<tqa_types::ScreenTarget>,
    /// Which of its captures the store kept.
    pub evidence: Option<ScreenEvidence>,
}
fn object_arguments(arguments: &Value) -> Result<Value, ToolError> {
    if arguments.is_object() {
        return Ok(arguments.clone());
    }
    let detail = if let Value::String(raw) = arguments {
        match serde_json::from_str::<Value>(raw) {
            Err(error) => format!("invalid JSON: {error}"),
            Ok(_) => "expected a JSON object".into(),
        }
    } else {
        "expected a JSON object".into()
    };
    Err(ToolError::InvalidArgs {
        path: "/".into(),
        detail,
    })
}

/// Runs a blocking query off the async threads, bounded by [`SPACE_QUERY_TIMEOUT`] on the run's clock. A query that
/// hangs or fails counts as `false`; its thread is left to finish on its own, since a blocking call can't be cancelled.
async fn bounded_query(
    query: impl FnOnce() -> bool + Send + 'static,
    sleeper: &dyn Sleeper,
) -> bool {
    let answer = tokio::task::spawn_blocking(query);
    tokio::select! {
        answer = answer => answer.unwrap_or(false),
        () = sleeper.sleep(SPACE_QUERY_TIMEOUT) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::bounded_query;
    use std::{future::Future, pin::Pin, sync::mpsc, time::Duration};
    use tqa_types::Sleeper;

    /// A deadline that has already passed.
    struct Expired;
    impl Sleeper for Expired {
        fn sleep(&self, _: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(std::future::ready(()))
        }
    }
    /// A deadline that never comes.
    struct Never;
    impl Sleeper for Never {
        fn sleep(&self, _: Duration) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
            Box::pin(std::future::pending())
        }
    }

    #[tokio::test]
    async fn core_u_049_the_space_query_is_bounded_and_only_a_real_answer_counts() {
        assert!(bounded_query(|| true, &Never).await);
        assert!(!bounded_query(|| false, &Never).await);
        // A query that hangs (a stalled storage mount) is abandoned at the deadline and counts as not full.
        let (release, hung) = mpsc::channel::<()>();
        assert!(
            !bounded_query(
                move || {
                    let _ = hung.recv();
                    true
                },
                &Expired
            )
            .await
        );
        drop(release);
    }
}
