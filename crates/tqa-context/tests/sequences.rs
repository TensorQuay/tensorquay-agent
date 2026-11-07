//! CTX-U-010 and CORE-U-038: replay must include every sequenced frame.
use std::error::Error;
use tqa_context::{ProjectionError, project, resume_point};
use tqa_types::{ErrorCode, Event, Seq, ServerFrame};

#[test]
fn ctx_u_010_core_u_038_gaps_fail_and_non_one_bases_are_allowed() -> Result<(), Box<dyn Error>> {
    let mut history = vec![ServerFrame::Event {
        seq: Seq::from(10),
        run_id: None,
        step: None,
        ts: "2026-09-22T00:00:00Z".into(),
        event: Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: "request".into(),
            attachments: vec![],
        },
    }];
    // A retired approval request from an older history still takes its place in the sequence (ADR-0035).
    history.push(ServerFrame::PermissionRequest { seq: Seq::from(11) });
    history.push(ServerFrame::Heartbeat {
        run_id: "run".parse()?,
    });
    history.push(ServerFrame::Error {
        target: None,
        seq: Some(Seq::from(12)),
        code: ErrorCode::Internal,
        message: "fixture".into(),
        retryable: false,
    });
    assert_eq!(project(&history)?.len(), 1);
    assert!(resume_point(&history).is_ok());
    history.remove(1);
    assert!(
        matches!(project(&history), Err(ProjectionError::Invalid { seq, .. }) if seq == Seq::from(12))
    );
    assert!(
        matches!(resume_point(&history), Err(ProjectionError::Invalid { seq, .. }) if seq == Seq::from(12))
    );
    Ok(())
}
