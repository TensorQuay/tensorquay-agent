//! CORE-U-045: the runner reports what the instruction slot was built from, once after `run.started` and once after
//! `run.resumed`, and never when the runtime supplied no report.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::{RunOutcome, Runner};
use tqa_tools::ToolRegistry;
use tqa_types::{ContextLoaded, Event, LoadedSkill, MAX_CONTEXT_LOADED_ITEMS, SkillTier};

fn report(skills: usize) -> ContextLoaded {
    ContextLoaded {
        skills: (0..skills)
            .map(|number| LoadedSkill {
                name: format!("skill-{number:03}"),
                tier: SkillTier::Install,
                description: "d".into(),
            })
            .collect(),
        ..ContextLoaded::default()
    }
}
async fn types(
    loaded: Option<ContextLoaded>,
    history: &[tqa_types::ServerFrame],
    message: bool,
) -> Result<Vec<Event>, Box<dyn std::error::Error>> {
    let mut h = Harness::new(vec![answer()], ToolRegistry::new())?;
    h.deps.prompt.loaded = loaded;
    h.sink_with(MemorySink::new(history));
    let sink = h.sink.clone();
    let outcome = Runner::new(h.deps)?
        .run(
            history,
            message.then(turn),
            CancellationToken::new(),
            CancellationToken::new(),
        )
        .await?;
    assert_eq!(outcome, RunOutcome::Completed);
    Ok(sink.events().split_off(history.len()))
}
fn names(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .take(4)
        .map(|event| {
            serde_json::to_value(event)
                .map(|value| value["type"].to_string())
                .unwrap_or_default()
        })
        .collect()
}

#[tokio::test]
async fn core_u_045_context_loaded_follows_run_started_once() -> TestResult {
    let events = types(Some(report(2)), &[], true).await?;
    assert_eq!(
        names(&events),
        [
            r#""message.user""#,
            r#""run.started""#,
            r#""context.loaded""#,
            r#""step.started""#
        ]
    );
    assert_eq!(events[2], report(2).into_event());
    let count = events
        .iter()
        .filter(|event| matches!(event, Event::ContextLoaded { .. }))
        .count();
    assert_eq!(count, 1);
    Ok(())
}

#[tokio::test]
async fn core_u_045_context_loaded_follows_run_resumed() -> TestResult {
    let history = history(vec![
        Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "question".into(),
            attachments: vec![],
        },
        Event::RunStarted {
            run_id: "r-1".parse()?,
        },
    ])?;
    let events = types(Some(report(1)), &history, false).await?;
    assert_eq!(
        names(&events)[..3],
        [
            r#""run.resumed""#,
            r#""context.loaded""#,
            r#""step.started""#
        ]
    );
    Ok(())
}

#[tokio::test]
async fn core_u_045_context_loaded_follows_the_start_of_a_journalled_message() -> TestResult {
    let history = history(vec![Event::MessageUser {
        lead_brief: None,
        team_wake: None,
        text: "question".into(),
        attachments: vec![],
    }])?;
    let events = types(Some(report(1)), &history, false).await?;
    assert_eq!(
        names(&events)[..3],
        [
            r#""run.started""#,
            r#""context.loaded""#,
            r#""step.started""#
        ]
    );
    Ok(())
}

#[tokio::test]
async fn core_u_045_no_report_no_event_and_lists_are_bounded() -> TestResult {
    let events = types(None, &[], true).await?;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::ContextLoaded { .. }))
    );
    let Event::ContextLoaded { skills, .. } = report(MAX_CONTEXT_LOADED_ITEMS + 1).into_event()
    else {
        return Err("expected context.loaded".into());
    };
    assert_eq!(skills.len(), MAX_CONTEXT_LOADED_ITEMS);
    Ok(())
}
