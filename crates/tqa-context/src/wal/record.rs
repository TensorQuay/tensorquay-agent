//! Bounded JSONL encoding and recovery; only a torn final line is discardable.
use super::{MAX_WAL_BYTES, Recovery, WalError};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
};
use tqa_types::{MAX_FRAME_BYTES, Seq, SeqChecker, ServerFrame};

const MAX_RECORD_BYTES: usize = MAX_FRAME_BYTES + 100;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    seq: Seq,
    frame: ServerFrame,
}

pub(super) fn sequence(frame: &ServerFrame) -> Result<Seq, WalError> {
    let seq = match frame {
        ServerFrame::Event { seq, .. }
        | ServerFrame::PermissionRequest { seq, .. }
        | ServerFrame::Error { seq: Some(seq), .. } => *seq,
        _ => return Err(WalError::InvalidFrame),
    };
    if seq == Seq::ZERO {
        return Err(WalError::InvalidFrame);
    }
    Ok(seq)
}
pub(super) fn check(previous: Option<Seq>, next: Seq) -> Result<(), WalError> {
    if let Some(previous) = previous {
        SeqChecker::new(previous).accept(next)?;
    }
    Ok(())
}
pub(super) fn encode(frame: &ServerFrame) -> Result<Vec<u8>, WalError> {
    let seq = sequence(frame)?;
    let encoded = serde_json::to_vec(frame).map_err(|_| WalError::InvalidFrame)?;
    ServerFrame::parse(&encoded).map_err(|_| WalError::InvalidFrame)?;
    let mut line = serde_json::to_vec(&Record {
        seq,
        frame: frame.clone(),
    })
    .map_err(|_| WalError::InvalidFrame)?;
    line.push(b'\n');
    Ok(line)
}
pub(super) fn recover(file: &mut File) -> Result<Recovery, WalError> {
    let length = file.metadata()?.len();
    if length > MAX_WAL_BYTES {
        return Err(WalError::Full);
    }
    let mut reader = BufReader::new(&mut *file);
    let mut records = Vec::new();
    let (mut valid_bytes, mut truncated_tail) = (0_u64, false);
    loop {
        let mut line = Vec::new();
        let count = Read::by_ref(&mut reader)
            .take(MAX_RECORD_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        if count > MAX_RECORD_BYTES {
            return Err(WalError::Corrupt);
        }
        if !line.ends_with(b"\n") {
            truncated_tail = true;
            break;
        }
        let record: Record = serde_json::from_slice(&line).map_err(|_| WalError::Corrupt)?;
        if sequence(&record.frame).map_err(|_| WalError::Corrupt)? != record.seq {
            return Err(WalError::Corrupt);
        }
        encode(&record.frame).map_err(|_| WalError::Corrupt)?;
        check(records.last().map(|(seq, _)| *seq), record.seq)?;
        records.push((record.seq, record.frame));
        valid_bytes += count as u64;
    }
    drop(reader);
    if truncated_tail {
        file.set_len(valid_bytes)?;
        file.sync_data()?;
    }
    file.seek(SeekFrom::End(0))?;
    Ok(Recovery {
        records,
        truncated_tail,
    })
}
