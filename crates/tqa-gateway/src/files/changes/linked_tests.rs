//! GW-I-084 (unit side): which copies a snapshot asks the runtime for, and in which batches.
use super::{BLOB_BATCH_BYTES, BLOB_BATCH_HASHES, batches, copies};
use crate::files::changes::{
    blobs::Blobs,
    manifest::{Manifest, Record},
};

fn record(size: u64, hash: &str, text: bool) -> Record {
    Record {
        size,
        stamp: (1, 0, 0, 0, size),
        hash: Some(hash.repeat(64)),
        text,
        copied: false,
    }
}

fn manifest(records: &[(&str, Record)]) -> Manifest {
    Manifest {
        files: records
            .iter()
            .map(|(path, record)| ((*path).to_owned(), record.clone()))
            .collect(),
        truncated: false,
    }
}

#[test]
fn gw_i_084_copies_are_asked_for_within_the_budget_once_each()
-> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let blobs = Blobs::new(temporary.path().to_path_buf());
    let stored = blobs.put(b"already")?;
    let mut previous = manifest(&[("kept.txt", record(4, "a", true))]);
    if let Some(kept) = previous.files.get_mut("kept.txt") {
        kept.copied = true;
    }
    let mut current = manifest(&[
        ("a.txt", record(10, "b", true)),
        ("b.txt", record(10, "b", true)),
        ("binary", record(10, "c", false)),
        ("huge.txt", record(BLOB_BATCH_BYTES + 1, "d", true)),
        ("kept.txt", record(4, "a", true)),
        ("over.txt", record(90, "e", true)),
        (
            "stored.txt",
            Record {
                hash: Some(stored.clone()),
                ..record(7, "f", true)
            },
        ),
    ]);
    let wanted = copies(&mut current, Some(&previous), 30, &blobs);
    // One request per new hash; the kept copy carries over; binary, oversized and over-budget files are not asked for.
    assert_eq!(wanted, [("b".repeat(64), 10)]);
    assert!(current.files["kept.txt"].copied && current.files["stored.txt"].copied);
    assert!(!current.files["over.txt"].copied && !current.files["huge.txt"].copied);
    Ok(())
}

#[test]
fn gw_i_084_copies_go_in_batches_bounded_by_count_and_bytes() {
    let many: Vec<(String, u64)> = (0..=BLOB_BATCH_HASHES)
        .map(|index| (format!("{index:064}"), 1))
        .collect();
    let by_count = batches(&many);
    assert_eq!(
        by_count.iter().map(|batch| batch.len()).collect::<Vec<_>>(),
        [BLOB_BATCH_HASHES, 1]
    );
    let large = BLOB_BATCH_BYTES / 2 + 1;
    let big: Vec<(String, u64)> = (0..3)
        .map(|index| (format!("{index:064}"), large))
        .collect();
    assert_eq!(batches(&big).len(), 3);
    assert!(batches(&[]).is_empty());
}
