//! Stagnation is rebuilt from completed steps and authoritative capped result content.
use super::session::{Execution, Stop};
use std::collections::BTreeSet;
use tqa_types::{BudgetKind, ErrorCode, Event, RunId, ServerFrame, ToolStatus};

const WARNING_STEPS: u64 = 6;
const STOP_STEPS: u64 = 12;

impl Execution<'_> {
    pub(super) async fn check_progress(&mut self) -> Result<(), Stop> {
        let run = self.run_id()?;
        let used = stagnant_steps(&self.frames, &run);
        let warned = self.frames.iter().any(|frame| {
            matches!(frame, ServerFrame::Event {
            run_id: Some(id), event: Event::BudgetWarning { budget: BudgetKind::NoProgress, .. }, ..
        } if *id == run)
        });
        if used >= WARNING_STEPS && !warned {
            self.emit(Event::BudgetWarning {
                budget: BudgetKind::NoProgress,
                used,
                limit: STOP_STEPS,
            })
            .await?;
        }
        if used >= STOP_STEPS {
            return Err(Stop::Failed(ErrorCode::NoProgress, "no_progress".into()));
        }
        Ok(())
    }
}

fn stagnant_steps(frames: &[ServerFrame], run: &RunId) -> u64 {
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    let mut stagnant = 0_u64;
    for frame in frames {
        let ServerFrame::Event {
            run_id: Some(id),
            event,
            ..
        } = frame
        else {
            continue;
        };
        if id != run {
            continue;
        }
        match event {
            Event::StepStarted { .. } => results.clear(),
            Event::ToolResult {
                status, content, ..
            } => results.push((*status, content.as_str())),
            Event::StepCompleted { .. } => {
                let new = results.iter().any(|(_, content)| !seen.contains(content));
                stagnant = advance_stagnation(stagnant, &results, new);
                seen.extend(results.iter().map(|(_, content)| *content));
            }
            _ => {}
        }
    }
    stagnant
}

fn advance_stagnation(stagnant: u64, results: &[(ToolStatus, &str)], changed: bool) -> u64 {
    if repeated_errors(results) {
        return stagnant;
    }
    if changed {
        0
    } else {
        stagnant.saturating_add(1)
    }
}

fn repeated_errors(results: &[(ToolStatus, &str)]) -> bool {
    results.first().is_some_and(|first| {
        first.0 == ToolStatus::Error && results.iter().all(|result| result == first)
    })
}
