//! Progress notices belong to an active run and never create a new user turn on resume.
use tqa_context::{ResumePoint, project, resume_point};
use tqa_types::{BudgetKind, Event, Seq, ServerFrame};

#[test]
fn core_e_037_progress_notice_rejects_an_absent_or_wrong_active_run()
-> Result<(), Box<dyn std::error::Error>> {
    let run: tqa_types::RunId = "run".parse()?;
    let start = ServerFrame::Event {
        seq: Seq::from(1),
        run_id: Some(run.clone()),
        step: None,
        ts: "2001-01-01T00:00:00Z".into(),
        event: Event::RunStarted {
            run_id: run.clone(),
        },
    };
    let warning = |run_id| ServerFrame::Event {
        seq: Seq::from(2),
        run_id,
        step: None,
        ts: "2001-01-01T00:00:00Z".into(),
        event: Event::BudgetWarning {
            budget: BudgetKind::NoProgress,
            used: 6,
            limit: 12,
        },
    };
    assert!(project(&[warning(Some(run.clone()))]).is_err());
    assert!(project(&[start.clone(), warning(None)]).is_err());
    assert!(project(&[start.clone(), warning(Some("other".parse()?))]).is_err());
    let frames = [start, warning(Some(run.clone()))];
    assert_eq!(project(&frames)?.len(), 1);
    assert_eq!(
        resume_point(&frames)?,
        ResumePoint::NextStep {
            run_id: run,
            step: 1
        }
    );
    Ok(())
}
