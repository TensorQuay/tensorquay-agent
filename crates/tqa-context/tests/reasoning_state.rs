//! CTX-U-016: opaque reasoning states attach to the authoritative response that produced them, and
//! compaction drops them with the turns it summarises.
#[path = "support/history.rs"]
mod support;
use serde_json::json;
use std::error::Error;
use support::{call, frames, model, result, start, step, text};
use tqa_context::{Message, ReasoningStateRecord, plan_compaction, project};
use tqa_types::{
    COMPACTION_VERSION, CompactionSource, Event, FinishReason, ToolStatus, WireProtocol,
};

type TestResult = Result<(), Box<dyn Error>>;

fn item(id: &str) -> String {
    json!({"type":"reasoning","id":id,"encrypted_content":"e30="}).to_string()
}
fn state(id: Option<&str>, route: &str) -> Result<Event, Box<dyn Error>> {
    let wire = match id {
        Some(id) => json!({"type":"reasoning.state","protocol":"openai_responses","route":route,
            "model":"fake-model","bytes":item(id).len(),"item":item(id)}),
        None => json!({"type":"reasoning.state","protocol":"openai_responses","route":route,
            "model":"fake-model","bytes":200_000}),
    };
    Ok(serde_json::from_value(wire)?)
}
fn record(id: Option<&str>, route: &str) -> ReasoningStateRecord {
    ReasoningStateRecord {
        protocol: WireProtocol::OpenAiResponses,
        route: route.into(),
        model: "fake-model".into(),
        item: id.map(item),
    }
}
fn assistant(messages: &[Message]) -> Result<&Message, Box<dyn Error>> {
    messages
        .iter()
        .find(|message| message.role == tqa_context::Role::Assistant)
        .ok_or_else(|| "no assistant message".into())
}

#[test]
fn ctx_u_016_states_attach_in_order_to_the_authoritative_attempt() -> TestResult {
    let history = frames(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "task".into(),
            attachments: vec![],
        },
        start("run")?,
        step(1),
        state(Some("superseded"), "local")?,
        text("discarded"),
        step(2),
        state(Some("first"), "local")?,
        state(None, "local")?,
        state(Some("third"), "backup")?,
        text("reading"),
        call("a")?,
        model(2),
        result("a")?,
    ]);
    let messages = project(&history)?;
    let message = assistant(&messages)?;
    assert_eq!(
        message.reasoning_states,
        [
            record(Some("first"), "local"),
            record(None, "local"),
            record(Some("third"), "backup"),
        ]
    );
    // Messages without states serialize as before.
    let plain = serde_json::to_value(&messages[0])?;
    assert!(plain.get("reasoning_states").is_none(), "{plain}");
    Ok(())
}

#[test]
fn ctx_u_016_a_state_outside_a_streaming_attempt_is_refused() -> TestResult {
    let after_model = frames(vec![
        start("run")?,
        step(1),
        call("a")?,
        model(1),
        state(Some("late"), "local")?,
    ]);
    assert!(project(&after_model).is_err());
    let outside_run = frames(vec![state(Some("stray"), "local")?]);
    assert!(project(&outside_run).is_err());
    Ok(())
}

/// A run of `steps` tool steps, each with one recorded state.
fn stateful_run(steps: u32) -> Result<Vec<Event>, Box<dyn Error>> {
    let mut events = vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "the task statement".into(),
            attachments: vec![],
        },
        start("long")?,
    ];
    for number in 1..=steps {
        let call_id: tqa_types::ToolCallId = format!("c{number}").parse()?;
        events.extend([
            Event::StepStarted {
                step: number,
                attempt: 1,
            },
            state(Some(&format!("s{number}")), "local")?,
            Event::ToolCall {
                call_id: call_id.clone(),
                name: "read_file".into(),
                arguments: json!({"path": format!("src/{number}.rs")}),
            },
            Event::ModelCompleted {
                step: number,
                attempt: 1,
                finish_reason: FinishReason::ToolCalls,
            },
            Event::ToolResult {
                call_id,
                status: ToolStatus::Ok,
                content: "ok".into(),
                target: None,
                evidence: None,
            },
            Event::StepCompleted {
                step: number,
                duration_ms: 0,
            },
        ]);
    }
    Ok(events)
}

#[test]
fn ctx_u_016_compaction_drops_the_states_of_summarised_turns_only() -> TestResult {
    let mut events = stateful_run(24)?;
    let before = project(&frames(events.clone()))?;
    let all: usize = before
        .iter()
        .map(|message| message.reasoning_states.len())
        .sum();
    assert_eq!(all, 24);
    let plan = plan_compaction(&frames(events.clone()))?.ok_or("nothing to compact")?;
    events.push(Event::Compaction {
        through_seq: plan.through_seq,
        source: CompactionSource::Fallback,
        version: COMPACTION_VERSION,
        summary: plan.fallback.clone(),
    });
    let after = project(&frames(events))?;
    let kept: Vec<&str> = after
        .iter()
        .flat_map(|message| &message.reasoning_states)
        .filter_map(|state| state.item.as_deref())
        .collect();
    let retained_steps = after
        .iter()
        .filter(|message| !message.tool_calls.is_empty())
        .count();
    assert!(retained_steps > 0 && retained_steps < 24);
    assert_eq!(
        kept.len(),
        retained_steps,
        "each retained turn keeps its state"
    );
    let last = item("s24");
    assert!(
        kept.contains(&last.as_str()),
        "the newest turn keeps its state"
    );
    assert!(
        !kept.contains(&item("s1").as_str()),
        "a summarised turn loses its state"
    );
    Ok(())
}
