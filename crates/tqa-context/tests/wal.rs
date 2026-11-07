//! CTX-U-007, CTX-E-008/011: durable tails, strict recovery and writer exclusion.
use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
};
use tqa_context::{Wal, WalError};
use tqa_types::{Event, Seq, ServerFrame};
fn frame(seq: u64) -> ServerFrame {
    ServerFrame::Event {
        seq: Seq::from(seq),
        run_id: None,
        step: None,
        ts: "2026-09-22T00:00:00Z".into(),
        event: Event::MessageUser {
            lead_brief: None,
            team_wake: None,
            text: format!("message {seq}"),
            attachments: vec![],
        },
    }
}

#[test]
fn ctx_u_007_thousand_records_and_torn_tail_recover_exactly() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("state");
    let (mut wal, recovery) = Wal::open(&state)?;
    assert!(recovery.records.is_empty() && !recovery.truncated_tail);
    for seq in 1..=1000 {
        wal.append(&frame(seq))?;
    }
    wal.sync()?;
    let path = state.join("events.jsonl");
    let length = fs::metadata(&path)?.len();
    drop(wal);
    OpenOptions::new()
        .write(true)
        .open(&path)?
        .set_len(length - 10)?;
    let (mut wal, recovery) = Wal::open(&state)?;
    assert!(recovery.truncated_tail);
    assert_eq!(recovery.records.len(), 999);
    assert_eq!(recovery.records[998], (Seq::from(999), frame(999)));
    wal.append(&frame(1000))?;
    drop(wal);
    let (_, recovery) = Wal::open(&state)?;
    assert_eq!(recovery.records.len(), 1000);
    assert!(!recovery.truncated_tail);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::metadata(state)?.permissions().mode() & 0o777, 0o700);
    }
    Ok(())
}

#[test]
fn ctx_u_007_gap_duplicate_zero_and_corruption_are_rejected() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let (mut wal, _) = Wal::open(directory.path())?;
    assert!(wal.append(&frame(0)).is_err());
    wal.append(&frame(41))?;
    for seq in [41, 43] {
        assert!(matches!(
            wal.append(&frame(seq)),
            Err(WalError::Sequence(_))
        ));
    }
    wal.append(&frame(42))?;
    assert!(
        wal.append(&ServerFrame::Heartbeat {
            run_id: "run".parse()?
        })
        .is_err()
    );
    drop(wal);
    let path = directory.path().join("events.jsonl");
    for content in [
        format!(
            "{}\n{}\n",
            serde_json::json!({"seq":1,"frame":frame(1)}),
            serde_json::json!({"seq":3,"frame":frame(3)})
        ),
        format!(
            "broken\n{}\n",
            serde_json::json!({"seq":1,"frame":frame(1)})
        ),
        format!("{}\n", serde_json::json!({"seq":2,"frame":frame(1)})),
        "broken\n".into(),
    ] {
        fs::write(&path, content)?;
        assert!(Wal::open(directory.path()).is_err());
    }
    fs::write(&path, "{\"seq\":")?;
    let (_, recovery) = Wal::open(directory.path())?;
    assert!(recovery.truncated_tail && recovery.records.is_empty());
    assert!(fs::read(path)?.is_empty());
    Ok(())
}

#[test]
fn ctx_e_008_exclusion_survives_atomic_truncation() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let (mut wal, _) = Wal::open(directory.path())?;
    for seq in 10..=13 {
        wal.append(&frame(seq))?;
    }
    assert!(matches!(Wal::open(directory.path()), Err(WalError::Locked)));
    assert_eq!(
        wal.records_after(Seq::from(11)),
        vec![(Seq::from(12), frame(12)), (Seq::from(13), frame(13))]
    );
    wal.truncate_through(Seq::from(11))?;
    assert!(matches!(Wal::open(directory.path()), Err(WalError::Locked)));
    drop(wal);
    let (mut wal, recovery) = Wal::open(directory.path())?;
    assert_eq!(recovery.records.len(), 2);
    assert_eq!(recovery.records[0].0, Seq::from(12));
    wal.truncate_through(Seq::from(13))?;
    assert!(wal.records_after(Seq::ZERO).is_empty());
    assert!(wal.append(&frame(13)).is_err());
    wal.append(&frame(14))?;
    wal.truncate_through(Seq::from(20))?;
    assert!(wal.append(&frame(20)).is_err());
    wal.append(&frame(21))?;
    drop(wal);
    let (_, recovery) = Wal::open(directory.path())?;
    assert_eq!(recovery.records, vec![(Seq::from(21), frame(21))]);
    Ok(())
}

#[test]
fn ctx_e_011_oversized_wal_and_invalid_storage_fail_closed() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("events.jsonl");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?
        .set_len(64 * 1024 * 1024 + 1)?;
    assert!(matches!(Wal::open(directory.path()), Err(WalError::Full)));
    fs::remove_file(path)?;
    let blocked = directory.path().join("file");
    fs::write(&blocked, "obstruction")?;
    assert!(Wal::open(&blocked).is_err());
    let (mut wal, _) = Wal::open(directory.path())?;
    wal.append(&frame(1))?;
    drop(wal);
    let mut file = OpenOptions::new()
        .append(true)
        .open(directory.path().join("events.jsonl"))?;
    file.write_all(b"bad final record\n")?;
    assert!(Wal::open(directory.path()).is_err());
    Ok(())
}

#[test]
fn ctx_u_007_wire_bounds_and_sequence_exhaustion_are_checked_before_append()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let (mut wal, _) = Wal::open(directory.path())?;
    let oversized = ServerFrame::Event {
        seq: Seq::from(1),
        run_id: None,
        step: None,
        ts: "2026-09-22T00:00:00Z".into(),
        event: Event::TextDelta {
            text: "x".repeat(tqa_types::MAX_FRAME_BYTES),
        },
    };
    assert!(matches!(
        wal.append(&oversized),
        Err(WalError::InvalidFrame)
    ));
    assert!(wal.records_after(Seq::ZERO).is_empty());
    wal.append(&frame(u64::MAX))?;
    assert!(matches!(
        wal.append(&frame(u64::MAX)),
        Err(WalError::Sequence(tqa_types::SeqError::Exhausted))
    ));
    Ok(())
}

#[test]
fn ctx_e_011_a_journal_path_that_is_not_a_regular_file_is_refused_as_corrupt()
-> Result<(), Box<dyn Error>> {
    // A directory, or a link to a file elsewhere, in place of the journal is never followed or overwritten.
    let directory = tempfile::tempdir()?;
    fs::create_dir(directory.path().join("events.jsonl"))?;
    assert!(matches!(
        Wal::open(directory.path()),
        Err(WalError::Corrupt)
    ));
    let linked = tempfile::tempdir()?;
    let outside = linked.path().join("outside.jsonl");
    fs::write(&outside, "")?;
    std::os::unix::fs::symlink(&outside, linked.path().join("events.jsonl"))?;
    assert!(matches!(Wal::open(linked.path()), Err(WalError::Corrupt)));
    assert!(
        fs::read(&outside)?.is_empty(),
        "the link's target is untouched"
    );
    Ok(())
}
