//! What a run has spent of its `computer` caps (ADR-0039 section 4), read back from its journal, so a run resumed after
//! an interruption (a new runtime, a new tool) carries on from its counts instead of starting them at zero. The journal
//! is the run's durable state; there is no second count to drift from it.
use std::collections::HashMap;

use tqa_types::{Event, RunId, ServerFrame, ToolStatus};

/// The refusals that come before an action is counted: the call never reached the screen.
const NOT_COUNTED: [&str; 3] = ["invalid_args:", "no_screenshot_yet:", "action_limit:"];

/// A run's spending against its caps.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Spent {
    /// Screenshots the model was sent.
    pub screenshots: u32,
    /// Actions that reached the screen.
    pub actions: u32,
}

impl Spent {
    /// What the run in progress in `history` (the latest `run.started` not yet ended) has spent: each screenshot that
    /// came back as one, and each acting call that got past its checks. Nothing for a history with no run going.
    #[must_use]
    pub fn from_journal(history: &[ServerFrame]) -> Self {
        let mut run: Option<RunId> = None;
        let mut calls: HashMap<String, String> = HashMap::new();
        let mut spent = Self::default();
        for frame in history {
            let ServerFrame::Event { event, run_id, .. } = frame else {
                continue;
            };
            match event {
                Event::RunStarted { run_id: started } => {
                    run = Some(started.clone());
                    calls.clear();
                    spent = Self::default();
                }
                Event::RunCompleted { run_id: ended }
                | Event::RunCancelled { run_id: ended }
                | Event::RunFailed { run_id: ended, .. }
                    if run.as_ref() == Some(ended) =>
                {
                    run = None;
                    spent = Self::default();
                }
                Event::ToolCall {
                    call_id,
                    name,
                    arguments,
                } if name == "computer" && run.is_some() && run_id.as_ref() == run.as_ref() => {
                    let action = arguments["action"].as_str().unwrap_or_default().to_owned();
                    calls.insert(call_id.to_string(), action);
                }
                Event::ToolResult {
                    call_id,
                    status,
                    content,
                    ..
                } => spent.count(calls.remove(call_id.as_str()).as_deref(), *status, content),
                _ => {}
            }
        }
        spent
    }

    /// Counts one result as the live tool counted it; a result of no `computer` call of this run counts nothing.
    fn count(&mut self, action: Option<&str>, status: ToolStatus, content: &str) {
        let Some(action) = action else {
            return;
        };
        if action == "screenshot" {
            if status == ToolStatus::Ok {
                self.screenshots = self.screenshots.saturating_add(1);
            }
            return;
        }
        if action == "observe" || NOT_COUNTED.iter().any(|code| content.starts_with(code)) {
            return;
        }
        self.actions = self.actions.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::Spent;
    use serde_json::{Value, json};
    use tqa_types::ServerFrame;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn frames(events: &[(Option<&str>, Value)]) -> Result<Vec<ServerFrame>, serde_json::Error> {
        events
            .iter()
            .enumerate()
            .map(|(index, (run, event))| {
                serde_json::from_value(
                    json!({"type": "event", "seq": index + 1, "run_id": run, "step": null,
                                              "ts": "2026-10-05T12:00:00Z", "event": event}),
                )
            })
            .collect()
    }

    fn call(id: &str, name: &str, action: &str) -> Value {
        json!({"type": "tool.call", "call_id": id, "name": name, "arguments": {"action": action}})
    }

    fn result(id: &str, content: &str) -> Value {
        json!({"type": "tool.result", "call_id": id, "status": "ok", "content": content})
    }

    // TEST: TOOL-U-045
    #[test]
    fn tool_u_045_the_runs_spending_is_read_back_from_its_journal() -> TestResult {
        let run = Some("r-2");
        let history = frames(&[
            (Some("r-1"), json!({"type": "run.started", "run_id": "r-1"})),
            (Some("r-1"), call("old", "computer", "screenshot")),
            (Some("r-1"), result("old", "Screenshot 1280×800, ...")),
            (
                Some("r-1"),
                json!({"type": "run.completed", "run_id": "r-1"}),
            ),
            (run, json!({"type": "run.started", "run_id": "r-2"})),
            (run, call("s1", "computer", "screenshot")),
            (run, result("s1", "Screenshot 1280×800, ...")),
            (run, call("o1", "computer", "observe")),
            (run, result("o1", "Snapshot 2 ...")),
            (run, call("k1", "computer", "key")),
            (run, result("k1", "Pressed Tab; no visible change.")),
            (run, call("c1", "computer", "click")),
            (
                run,
                result("c1", "stale_ref: e9 isn't in snapshot 2; observe again"),
            ),
            (run, call("c2", "computer", "click")),
            (
                run,
                result(
                    "c2",
                    "invalid_args: /x: the point is outside the latest screenshot (1280×800)",
                ),
            ),
            (run, call("f1", "read_file", "x")),
            (run, result("f1", "text")),
        ])?;
        assert_eq!(
            Spent::from_journal(&history),
            Spent {
                screenshots: 1,
                actions: 2
            },
            "this run's own, counted as the live tool counts: the key and the stale click (spent before the ref is resolved), not the refused point"
        );
        let mut ended = history.clone();
        ended.extend(frames(&[(
            run,
            json!({"type": "run.completed", "run_id": "r-2"}),
        )])?);
        assert_eq!(
            Spent::from_journal(&ended),
            Spent::default(),
            "nothing once the run is over"
        );
        Ok(())
    }
}
