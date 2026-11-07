//! `done` and the administrator's acceptance check (ADR-0034 point 10, GW-I-198). When a check is set, the lead's
//! `done` runs it first in the lead's workspace through the run's own shell tool, so it runs as any shell command does
//! (the workspace as its directory, the shell's environment, its process group stopped with it), bounded by
//! [`ACCEPTANCE_TIMEOUT`] rather than the shell's own limit. `done` sends what it did with the summary, and the gateway
//! holds `done` to it: a failed check is refused with its exit and the end of its output, and the team goes on.
use super::tools::{Refused, Shared, identity, object, present, refusal, settle, text};
use crate::{PreparedAction, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tqa_types::{CheckReport, MAX_CHECK_OUTPUT_BYTES, last_bytes};

/// The longest an acceptance check may run: long enough for a project's test suite, unlike the shell's own limit.
pub const ACCEPTANCE_TIMEOUT: Duration = Duration::from_mins(10);

/// Refusals from asking for the check that `done` itself answers better (the lead's unread messages ride a
/// `team_ended`), and an older gateway's `not_found`: `done` goes on without a check, and the gateway decides.
const DONE_ANSWERS: [&str; 5] = [
    "not_found",
    "teams_disabled",
    "not_a_member",
    "not_lead",
    "team_ended",
];

/// How a lead's `done` runs the acceptance check: the run's shell tool, and the time it may take.
#[derive(Clone)]
pub struct AcceptanceShell {
    shell: Arc<dyn Tool>,
    timeout: Duration,
}

impl AcceptanceShell {
    /// Runs the check with `shell`, the run's shell tool, for at most [`ACCEPTANCE_TIMEOUT`].
    #[must_use]
    pub fn new(shell: Arc<dyn Tool>) -> Self {
        Self {
            shell,
            timeout: ACCEPTANCE_TIMEOUT,
        }
    }

    /// The same, for at most `timeout` (tests).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Runs `command` in the workspace root: its exit, whether it was stopped at the time limit, and the end of what
    /// it printed. Only a Stop ends it without a report.
    async fn run(&self, command: &str, cx: &ToolContext) -> Result<CheckReport, ToolError> {
        let mut bounded = cx.clone();
        // The shell's own output budget stays: up to it, the output arrives whole and its last 4 KiB are exact (a
        // smaller budget would keep a head and lose the middle of the end); past it, the shell marks the cut.
        bounded.limits.shell_timeout = self.timeout;
        let args = json!({"command": command, "cwd": null, "timeout_ms": null});
        let report = |exit_code, timed_out, output: &str| CheckReport {
            command: command.to_owned(),
            exit_code,
            timed_out,
            output: last_bytes(output, MAX_CHECK_OUTPUT_BYTES).to_owned(),
        };
        match self.shell.call(args, &bounded).await {
            Ok(output) => Ok(report(output.exit_code, false, &output.text)),
            Err(ToolError::Timeout) => Ok(report(None, true, "")),
            Err(ToolError::Cancelled) => Err(ToolError::Cancelled),
            Err(error) => Ok(report(None, false, &error.model_message())),
        }
    }

    fn limit(&self) -> String {
        let seconds = self.timeout.as_secs().max(1);
        match (seconds / 60, seconds) {
            (1, 60) => "1 minute".to_owned(),
            (minutes, _) if seconds.is_multiple_of(60) => format!("{minutes} minutes"),
            (_, 1) => "1 second".to_owned(),
            _ => format!("{seconds} seconds"),
        }
    }
}

/// `done`: ends the team's work with a summary, after the administrator's acceptance check when one is set; then the
/// helpers stop and the end note arrives (ADR-0034 point 10).
pub(super) struct Done {
    spec: ToolSpec,
    shared: Shared,
    check: Option<AcceptanceShell>,
}

impl Done {
    pub(super) fn new(shared: Shared, check: Option<AcceptanceShell>) -> Self {
        let parameters = object(&[(
            "summary",
            text("What the team did and where its results are, for the person."),
        )]);
        Self {
            spec: super::super::specification(
                "done",
                "Finish the team's work once you have checked each report: every helper stops, and the person gets your summary with the end note.",
                parameters,
                false,
            ),
            shared,
            check,
        }
    }

    async fn run(
        &self,
        args: Value,
        call: (&str, &str),
        cx: &ToolContext,
    ) -> Result<ToolOutput, Refused> {
        let mut body = present(args);
        let asked = tokio::select! {
            biased;
            () = cx.cancel.cancelled() => return Err(ToolError::Cancelled.into()),
            asked = self.shared.client.acceptance(call.1, &cx.cancel) => asked,
        };
        let command = match asked {
            Ok(command) => command,
            Err(failure) if DONE_ANSWERS.contains(&failure.code.as_str()) => None,
            Err(failure) => return Err(refusal(&failure)),
        };
        if let Some(command) = command {
            let Some(check) = &self.check else {
                return Err(ToolError::channel(
                    "acceptance_failed",
                    "the acceptance check can't run: this task has no shell tool. The administrator can change the \
                     check or the tools this task gets.",
                )
                .into());
            };
            let report = check.run(&command, cx).await?;
            body["check"] =
                serde_json::to_value(&report).map_err(|_| ToolError::Io("check report".into()))?;
        }
        let posted = tokio::select! {
            biased;
            () = cx.cancel.cancelled() => return Err(ToolError::Cancelled.into()),
            posted = self.shared.client.done(&body, call, &cx.cancel) => posted,
        };
        match posted {
            Ok(_) => Ok(ToolOutput::text(
                "Your helpers are stopping, and the end note follows. Finish your answer to the person.",
            )),
            // The check's output is the result, as a tool error is capped and holds no content; the team goes on.
            Err(failure) if failure.code == "acceptance_failed" => match &failure.details {
                Some(details) => Ok(self.failed(details)),
                None => Err(refusal(&failure)),
            },
            Err(failure) => Err(refusal(&failure)),
        }
    }

    /// A refused `done` whose check ran and failed, as the gateway's details say: how it ended and the end of its
    /// output, so the lead can fix the work and call `done` again.
    fn failed(&self, details: &Value) -> ToolOutput {
        let exit_code = details["exit_code"]
            .as_i64()
            .and_then(|code| i32::try_from(code).ok());
        let how = match (exit_code, details["timed_out"] == true) {
            (_, true) => format!(
                "stopped after {}",
                self.check
                    .as_ref()
                    .map_or_else(|| "its time limit".to_owned(), AcceptanceShell::limit)
            ),
            (Some(code), false) => format!("exit {code}"),
            (None, false) => "it didn't finish".to_owned(),
        };
        let output = details["output"].as_str().unwrap_or_default();
        let shown = if output.is_empty() {
            "It printed nothing.".to_owned()
        } else {
            format!("Its last output:\n{output}")
        };
        let mut result = ToolOutput::text(format!(
            "acceptance_failed: the acceptance check failed ({how}), so the team goes on. Fix what it found, then \
             call done again. {shown}"
        ));
        result.exit_code = exit_code;
        result
    }
}

#[async_trait]
impl Tool for Done {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    fn offered(&self) -> bool {
        self.shared.leading.load(Ordering::SeqCst)
    }
    fn null_means_absent(&self) -> bool {
        true
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        settle(self.run(args, ("", ""), cx).await)
    }
    async fn call_prepared(
        &self,
        action: PreparedAction,
        cx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let (key, run) = identity(&action);
        let (_, args) = action.into_call();
        settle(self.run(args, (&key, &run), cx).await)
    }
}
