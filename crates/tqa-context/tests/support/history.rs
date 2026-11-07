//! Small deterministic histories shared by projection and resume tests.
use tqa_types::{Event, FinishReason, InvalidId, RunId, Seq, ServerFrame, ToolStatus};
pub fn start(id: &str) -> Result<Event, InvalidId> {
    Ok(Event::RunStarted {
        run_id: id.parse()?,
    })
}
pub fn step(attempt: u32) -> Event {
    Event::StepStarted { step: 1, attempt }
}
pub fn model(attempt: u32) -> Event {
    Event::ModelCompleted {
        step: 1,
        attempt,
        finish_reason: FinishReason::ToolCalls,
    }
}
pub fn call(id: &str) -> Result<Event, InvalidId> {
    Ok(Event::ToolCall {
        call_id: id.parse()?,
        name: "read_file".into(),
        arguments: serde_json::json!({"path":"file"}),
    })
}
pub fn result(id: &str) -> Result<Event, InvalidId> {
    Ok(Event::ToolResult {
        call_id: id.parse()?,
        status: ToolStatus::Ok,
        content: format!("result {id}"),
        target: None,
        evidence: None,
    })
}
pub fn text(text: &str) -> Event {
    Event::TextDelta { text: text.into() }
}
pub fn frames(events: Vec<Event>) -> Vec<ServerFrame> {
    let (mut run, mut step): (Option<RunId>, Option<u32>) = (None, None);
    events
        .into_iter()
        .enumerate()
        .map(|(index, event)| {
            if let Event::RunStarted { run_id } = &event {
                run = Some(run_id.clone());
                step = None;
            }
            if let Event::StepStarted { step: number, .. } = &event {
                step = Some(*number);
            }
            let (run_id, envelope_step) = if matches!(event, Event::MessageUser { .. }) {
                (None, None)
            } else {
                (run.clone(), step)
            };
            ServerFrame::Event {
                seq: Seq::from(index as u64 + 1),
                run_id,
                step: envelope_step,
                ts: "2026-09-22T00:00:00Z".into(),
                event,
            }
        })
        .collect()
}
