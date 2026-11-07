use super::*;
use tqa_types::Event;

#[test]
fn ctx_e_011_failed_or_short_append_cannot_be_emitted() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let frame = ServerFrame::Event {
        seq: Seq::from(1),
        run_id: None,
        step: None,
        ts: "2026-09-22T00:00:00Z".into(),
        event: Event::TextDelta {
            text: "record".into(),
        },
    };
    let (mut wal, _) = Wal::open(directory.path())?;
    for partial in [false, true] {
        let mut emitted = false;
        let result = wal.append_with(&frame, |file, bytes| {
            if partial {
                file.write(&bytes[..10])
            } else {
                Err(std::io::Error::from(std::io::ErrorKind::StorageFull))
            }
        });
        if result.is_ok() {
            emitted = true;
        }
        assert!(matches!(result, Err(WalError::Io(_))));
        assert!(!emitted);
        assert!(wal.records_after(Seq::ZERO).is_empty());
        assert_eq!(
            fs::metadata(directory.path().join("events.jsonl"))?.len(),
            0
        );
    }
    wal.append(&frame)?;
    wal.bytes = MAX_WAL_BYTES;
    assert!(matches!(
        wal.append(&ServerFrame::Event {
            seq: Seq::from(2),
            run_id: None,
            step: None,
            ts: "2026-09-22T00:00:00Z".into(),
            event: Event::TextDelta {
                text: "record".into()
            }
        }),
        Err(WalError::Full)
    ));
    assert_eq!(wal.records_after(Seq::ZERO).len(), 1);
    Ok(())
}

#[test]
fn ctx_e_011_failed_rollback_requires_recovery() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let (mut wal, _) = Wal::open(directory.path())?;
    let frame = ServerFrame::Event {
        seq: Seq::from(1),
        run_id: None,
        step: None,
        ts: "2026-09-22T00:00:00Z".into(),
        event: Event::TextDelta {
            text: "record".into(),
        },
    };
    let result = wal.append_with(&frame, |file, _| {
        *file = File::open(directory.path().join("events.jsonl"))?;
        Err(std::io::ErrorKind::StorageFull.into())
    });
    assert!(matches!(result, Err(WalError::Io(_))));
    assert!(matches!(wal.append(&frame), Err(WalError::NeedsRecovery)));
    assert!(matches!(wal.sync(), Err(WalError::NeedsRecovery)));
    assert!(matches!(
        wal.truncate_through(Seq::from(1)),
        Err(WalError::NeedsRecovery)
    ));
    drop(wal);
    let (mut wal, recovery) = Wal::open(directory.path())?;
    assert!(recovery.records.is_empty());
    wal.append(&frame)?;
    Ok(())
}
