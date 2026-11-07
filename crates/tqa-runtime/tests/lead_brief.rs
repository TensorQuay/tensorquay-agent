//! RT-I-039, through the runtime (ADR-0034, protocol revision 12): an agent team member's first turn carries the lead's
//! brief. The journalled `message.user` keeps it, so a client shows it as the brief from the lead, and the model gets
//! the gateway's frame and then the brief inside the untrusted-content marker; a person's message has neither. RT-I-044
//! (revision 13): the gateway's wake of a team reader is journalled with its marker (the reader's place), its
//! sentence unchanged.
#[path = "support/fixture.rs"]
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_runtime::Runtime;
use tqa_types::{Brief, ClientFrame, Event, LeadBrief, ServerFrame, TeamWake, WorkspaceMode};

const FRAME: &str = "You are researcher, a helper of this task's lead. Report with send_to_lead.";

fn lead() -> LeadBrief {
    LeadBrief {
        role: "researcher".into(),
        workspace: WorkspaceMode::MyFiles,
        brief: Brief {
            objective: "Find the Q3 revenue by region.".into(),
            output: "A table.".into(),
            sources: Some("reports/ only".into()),
            boundaries: "Don't edit the reports.".into(),
        },
    }
}

/// The journalled `message.user` and the first model request's body for a run on `message`.
async fn run(message: ClientFrame) -> Result<(Event, String), Box<dyn std::error::Error>> {
    let f = Fixture::new(vec![answer()]).await?;
    let runtime = Runtime::new(f.deps()?)?;
    let shutdown = CancellationToken::new();
    let cancel = shutdown.clone();
    let task = tokio::spawn(async move { runtime.run(cancel).await });
    f.gateway.set_bound(true);
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(message).await?;
    let result = f.wait_event("run.completed").await;
    stop(shutdown, task).await?;
    result?;
    let journalled = f
        .gateway
        .stored()
        .into_iter()
        .find_map(|frame| match frame {
            ServerFrame::Event {
                event: event @ Event::MessageUser { .. },
                ..
            } => Some(event),
            _ => None,
        })
        .ok_or("no message.user")?;
    let prompt = f.gateway.model_requests()[0].body.to_string();
    f.gateway.shutdown().await?;
    Ok((journalled, prompt))
}

// TEST: RT-I-039
#[tokio::test]
async fn rt_i_039_a_members_first_turn_keeps_the_lead_as_its_source() -> TestResult {
    let (journalled, prompt) = run(ClientFrame::UserMessage {
        text: FRAME.into(),
        attachments: vec![],
        host_keys: vec![],
        lead_brief: Some(lead()),
        team_wake: None,
    })
    .await?;
    let Event::MessageUser {
        text, lead_brief, ..
    } = journalled
    else {
        return Err("not a message".into());
    };
    assert_eq!(
        lead_brief,
        Some(lead()),
        "the stored event names the lead's brief"
    );
    assert!(text.starts_with(FRAME));
    assert!(text.contains(
        "<untrusted-content source=\"lead\" role=\"researcher\" workspace=\"my_files\" id=\""
    ));
    assert!(text.contains("Sources: reports/ only"));
    // The model reads what was journalled: the frame, then the brief inside its block.
    assert!(
        prompt.contains("untrusted-content source=\\\"lead\\\""),
        "{prompt}"
    );
    assert!(prompt.contains("Find the Q3 revenue by region."));
    Ok(())
}

// TEST: RT-I-039
#[tokio::test]
async fn rt_i_039_a_persons_message_has_no_lead_and_no_block() -> TestResult {
    let (journalled, prompt) = run(ClientFrame::UserMessage {
        text: "question".into(),
        attachments: vec![],
        host_keys: vec![],
        lead_brief: None,
        team_wake: None,
    })
    .await?;
    assert!(matches!(
        &journalled,
        Event::MessageUser { text, lead_brief: None, team_wake: None, .. } if text == "question"
    ));
    assert!(!prompt.contains("untrusted-content source=\\\"lead\\\""));
    Ok(())
}

// TEST: RT-I-044
#[tokio::test]
async fn rt_i_044_a_wake_is_journalled_with_its_marker_and_the_model_reads_its_sentence()
-> TestResult {
    let sentence = "New messages from your helpers since message 3. Read them with read_reports.";
    let wake = TeamWake { after: 3 };
    let (journalled, prompt) = run(ClientFrame::UserMessage {
        text: sentence.into(),
        attachments: vec![],
        host_keys: vec![],
        lead_brief: None,
        team_wake: Some(wake),
    })
    .await?;
    let Event::MessageUser {
        text,
        lead_brief,
        team_wake,
        ..
    } = journalled
    else {
        return Err("not a message".into());
    };
    assert_eq!(
        team_wake,
        Some(wake),
        "the stored event marks the gateway's wake"
    );
    assert_eq!((text.as_str(), lead_brief), (sentence, None));
    assert!(prompt.contains(sentence), "{prompt}");
    Ok(())
}
