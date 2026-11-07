//! Replay page bounds retain contiguous ordering before and after acknowledged-prefix truncation.
use tqa_context::Wal;
use tqa_types::{Event, SandboxState, Seq, ServerFrame};

#[test]
fn rt_i_011_wal_pages_preserve_every_frame_and_obey_the_requested_bound()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let (mut wal, _) = Wal::open(directory.path())?;
    for number in 1..=129 {
        wal.append(&ServerFrame::Event {
            seq: Seq::from(number),
            run_id: None,
            step: None,
            ts: "2001-01-01T00:00:00Z".into(),
            event: Event::SandboxState {
                state: SandboxState::Idle,
            },
        })?;
    }
    for watermark in [0, 1, 63, 64, 128, 129] {
        for limit in [1, 2, 64, 1000] {
            let expected = wal.records_after(Seq::from(watermark));
            assert_eq!(collect_pages(&wal, Seq::from(watermark), limit), expected);
        }
    }
    assert!(wal.records_page(Seq::ZERO, 0).is_empty());
    wal.truncate_through(Seq::from(64))?;
    let page = wal.records_page(Seq::ZERO, 64);
    assert_eq!(page.first().map(|(seq, _)| *seq), Some(Seq::from(65)));
    assert_eq!(page.last().map(|(seq, _)| *seq), Some(Seq::from(128)));
    assert_eq!(wal.records_page(Seq::from(128), 64).len(), 1);
    Ok(())
}

fn collect_pages(wal: &Wal, mut cursor: Seq, limit: usize) -> Vec<(Seq, ServerFrame)> {
    let mut records = Vec::new();
    loop {
        let page = wal.records_page(cursor, limit);
        assert!(page.len() <= limit);
        let Some((last, _)) = page.last() else {
            return records;
        };
        cursor = *last;
        records.extend(page);
    }
}
