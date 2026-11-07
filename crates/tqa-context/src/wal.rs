//! Runtime write-ahead storage in a caller-supplied private state directory.
mod error;
mod record;
mod storage;
pub use error::WalError;
use std::{
    fs::{self, File},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use tqa_types::{Event, Seq, ServerFrame};

/// Maximum persisted bytes in one runtime WAL; acknowledged prefixes can be removed.
pub const MAX_WAL_BYTES: u64 = 64 * 1024 * 1024;

/// Whether to sync before acknowledging the latest appended event: at step completion,
/// at any terminal run event, or after 100 records since the last successful sync.
/// `unsynced_records` includes the event just appended, and resets only after a successful sync.
#[must_use]
pub fn wal_should_sync(event: &Event, unsynced_records: usize) -> bool {
    matches!(
        event,
        Event::StepCompleted { .. }
            | Event::RunCompleted { .. }
            | Event::RunFailed { .. }
            | Event::RunCancelled { .. }
    ) || unsynced_records >= 100
}

/// Valid records recovered from the retained tail and whether its final partial line was removed.
#[derive(Clone, Debug, PartialEq)]
pub struct Recovery {
    /// Retained positive contiguous sequences with their original frames.
    pub records: Vec<(Seq, ServerFrame)>,
    /// True when recovery truncated an unterminated final line.
    pub truncated_tail: bool,
}

/// A single writer whose lock survives atomic replacement of its data file.
/// Operations are synchronous; the runtime should call them from its blocking worker.
#[derive(Debug)]
pub struct Wal {
    directory: PathBuf,
    _lock: File,
    file: File,
    records: Vec<(Seq, ServerFrame)>,
    last: Option<Seq>,
    bytes: u64,
    needs_recovery: bool,
}
impl Wal {
    /// Opens `events.jsonl` under a private state directory, outside the workspace by caller contract.
    /// An existing tail may start at any positive sequence; later records must be contiguous.
    ///
    /// # Errors
    /// Rejects competing writers, complete corrupt records, gaps, oversized files and I/O failures.
    pub fn open(directory: &Path) -> Result<(Self, Recovery), WalError> {
        let (lock, mut file) = storage::open(directory)?;
        let recovery = record::recover(&mut file)?;
        let bytes = file.metadata()?.len();
        let wal = Self {
            directory: directory.into(),
            _lock: lock,
            file,
            records: recovery.records.clone(),
            last: recovery.records.last().map(|(seq, _)| *seq),
            bytes,
            needs_recovery: false,
        };
        Ok((wal, recovery))
    }
    /// Writes one entire JSON line with one write call before the caller may emit the frame.
    ///
    /// # Errors
    /// Fails on invalid frames, gaps, capacity, short writes or I/O; failed records are never cached.
    pub fn append(&mut self, frame: &ServerFrame) -> Result<(), WalError> {
        self.append_with(frame, Write::write)
    }
    fn append_with(
        &mut self,
        frame: &ServerFrame,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<usize>,
    ) -> Result<(), WalError> {
        if self.needs_recovery {
            return Err(WalError::NeedsRecovery);
        }
        let seq = record::sequence(frame)?;
        record::check(self.last, seq)?;
        let line = record::encode(frame)?;
        if self.bytes.saturating_add(line.len() as u64) > MAX_WAL_BYTES {
            return Err(WalError::Full);
        }
        let result = write(&mut self.file, &line).and_then(|written| {
            if written == line.len() {
                Ok(())
            } else {
                Err(std::io::ErrorKind::WriteZero.into())
            }
        });
        if let Err(error) = result {
            self.needs_recovery = self
                .file
                .set_len(self.bytes)
                .and_then(|()| self.file.seek(SeekFrom::Start(self.bytes)))
                .is_err();
            return Err(error.into());
        }
        self.bytes += line.len() as u64;
        self.last = Some(seq);
        self.records.push((seq, frame.clone()));
        Ok(())
    }
    /// Flushes file data to storage at a runtime-selected durability boundary.
    ///
    /// # Errors
    /// Returns an I/O failure or refuses a handle requiring recovery.
    pub fn sync(&self) -> Result<(), WalError> {
        if self.needs_recovery {
            return Err(WalError::NeedsRecovery);
        }
        Ok(self.file.sync_data()?)
    }
    /// Returns retained frames strictly after the gateway's last acknowledged sequence.
    #[must_use]
    pub fn records_after(&self, seq: Seq) -> Vec<(Seq, ServerFrame)> {
        self.records_page(seq, usize::MAX)
    }
    /// Copies at most `limit` retained frames after the cursor, in sequence order.
    /// Zero returns an empty page. Continue from the last returned sequence to replay the whole tail.
    #[must_use]
    pub fn records_page(&self, seq: Seq, limit: usize) -> Vec<(Seq, ServerFrame)> {
        let start = self.records.partition_point(|(number, _)| *number <= seq);
        self.records[start..].iter().take(limit).cloned().collect()
    }
    /// Atomically removes an acknowledged prefix while retaining the live sequence high-water mark.
    /// An empty reopened WAL takes its first sequence from gateway history, not a local checkpoint.
    ///
    /// # Errors
    /// Returns storage failures and refuses a handle requiring recovery.
    pub fn truncate_through(&mut self, seq: Seq) -> Result<(), WalError> {
        if self.needs_recovery {
            return Err(WalError::NeedsRecovery);
        }
        let records = self.records_after(seq);
        let (file, stage) = storage::stage(&self.directory, &records)?;
        if let Err(error) = fs::rename(&stage, self.directory.join("events.jsonl")) {
            fs::remove_file(stage)?;
            return Err(error.into());
        }
        self.file = file;
        self.bytes = self.file.metadata()?.len();
        self.records = records;
        self.last = self.last.max((seq != Seq::ZERO).then_some(seq));
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
