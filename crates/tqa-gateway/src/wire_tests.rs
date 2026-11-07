use super::{db_sequence, run_transition, sequence, validated};
use crate::GatewayError;
use tqa_types::{ErrorCode, Event, SandboxState, ServerFrame};

#[test]
fn gw_i_005_only_valid_sequenced_frames_enter_the_journal() -> Result<(), Box<dyn std::error::Error>>
{
    let frame = ServerFrame::Error {
        seq: Some(1.into()),
        code: ErrorCode::Io,
        message: "synthetic".into(),
        retryable: false,
        target: None,
    };
    assert_eq!(sequence(&validated(&frame)?), Some(1.into()));
    assert_eq!(db_sequence(1.into())?, 1);
    assert!(db_sequence(u64::MAX.into()).is_err());
    let control = ServerFrame::Error {
        seq: None,
        code: ErrorCode::Io,
        message: String::new(),
        retryable: false,
        target: None,
    };
    assert_eq!(sequence(&control), None);
    let huge = ServerFrame::Error {
        seq: None,
        code: ErrorCode::Io,
        message: "x".repeat(tqa_types::MAX_FRAME_BYTES),
        retryable: false,
        target: None,
    };
    assert_eq!(
        validated(&huge),
        Err(GatewayError::Protocol(ErrorCode::InvalidFrame))
    );
    Ok(())
}

#[test]
fn gw_i_035_only_run_lifecycle_events_move_the_client_sandbox_state()
-> Result<(), Box<dyn std::error::Error>> {
    let run_id: tqa_types::RunId = "r-1".parse()?;
    let cases = [
        (
            Event::RunStarted {
                run_id: run_id.clone(),
            },
            Some(SandboxState::Busy),
        ),
        (
            Event::RunResumed {
                run_id: run_id.clone(),
                from_step: 2,
            },
            Some(SandboxState::Busy),
        ),
        (
            Event::RunCompleted {
                run_id: run_id.clone(),
            },
            Some(SandboxState::Idle),
        ),
        (
            Event::RunCancelled {
                run_id: run_id.clone(),
            },
            Some(SandboxState::Idle),
        ),
        (
            Event::RunFailed {
                run_id,
                reason: "synthetic".into(),
                code: ErrorCode::Internal,
                provider_message: None,
            },
            Some(SandboxState::Idle),
        ),
        (
            Event::TextDelta {
                text: "progress is liveness only".into(),
            },
            None,
        ),
    ];
    for (event, expected) in cases {
        let frame = ServerFrame::Event {
            seq: 1.into(),
            run_id: None,
            step: None,
            ts: "2026-01-01T00:00:00Z".into(),
            event,
        };
        assert_eq!(run_transition(&frame), expected, "{frame:?}");
    }
    assert_eq!(
        run_transition(&ServerFrame::Heartbeat {
            run_id: "r-1".parse()?
        }),
        None
    );
    Ok(())
}
