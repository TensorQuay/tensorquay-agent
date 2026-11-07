//! The shared durability policy counts the record just appended, before acknowledging it.
use tqa_context::wal_should_sync;
use tqa_types::{ErrorCode, Event};

#[test]
fn ctx_u_007_wal_sync_policy_table() -> Result<(), Box<dyn std::error::Error>> {
    let run_id: tqa_types::RunId = "sync-run".parse()?;
    let cases = [
        (
            Event::TextDelta {
                text: "delta".into(),
            },
            false,
        ),
        (
            Event::RunStarted {
                run_id: run_id.clone(),
            },
            false,
        ),
        (
            Event::StepStarted {
                step: 1,
                attempt: 1,
            },
            false,
        ),
        (
            Event::StepCompleted {
                step: 1,
                duration_ms: 1,
            },
            true,
        ),
        (
            Event::RunCompleted {
                run_id: run_id.clone(),
            },
            true,
        ),
        (
            Event::RunCancelled {
                run_id: run_id.clone(),
            },
            true,
        ),
        (
            Event::RunFailed {
                run_id,
                reason: "failure".into(),
                code: ErrorCode::Io,
                provider_message: None,
            },
            true,
        ),
    ];
    for (event, boundary) in cases {
        for records in [0, 1, 98, 99, 100, 101, usize::MAX] {
            assert_eq!(
                wal_should_sync(&event, records),
                boundary || records >= 100,
                "event={event:?}, unsynced records including current={records}"
            );
        }
    }
    Ok(())
}
