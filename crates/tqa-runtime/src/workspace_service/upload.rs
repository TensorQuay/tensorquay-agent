//! Uploads (ADR-0030, RT-I-026): each chunk is appended to the upload's hidden part file in `uploads/` only where the
//! part ends; the last one links it to the first free name, as the gateway stores uploads on Docker (shared naming in
//! `tqa-tools`). The first chunk of any upload removes parts nobody has written for a while: abandoned uploads.
use std::{
    os::{fd::AsFd, unix::fs::FileExt},
    path::Path,
    time::{Duration, SystemTime},
};

use base64::Engine as _;
use tqa_tools::workspace_tree::{
    TreeError, path as tree,
    upload::{self, DIRECTORY, TEMPORARY_PREFIX},
};
use tqa_types::{UploadedFile, WorkspaceErrorCode, WorkspaceResult};

use super::{error, failed};

/// A part not written for this long belongs to an abandoned upload (a chunk arrives within the gateway's I/O bound).
const STALE: Duration = Duration::from_mins(10);

/// One chunk of an upload.
pub(super) struct Chunk<'a> {
    pub upload_id: u64,
    pub name: &'a str,
    pub offset: u64,
    pub data: &'a str,
    pub last: bool,
}

/// Writes one chunk; the answer is the part's size, and on the last chunk the stored file.
pub(super) fn upload(root: &Path, chunk: &Chunk<'_>) -> WorkspaceResult {
    written(root, chunk).unwrap_or_else(|refusal| refusal)
}

fn written(root: &Path, chunk: &Chunk<'_>) -> Result<WorkspaceResult, WorkspaceResult> {
    // The gateway sends sanitised names; anything else did not come from it.
    if upload::sanitise(chunk.name) != chunk.name {
        return Err(error(
            WorkspaceErrorCode::InvalidPath,
            "the name is not sanitised",
        ));
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(chunk.data)
        .map_err(|_| error(WorkspaceErrorCode::Internal, "the chunk is not base64"))?;
    let root = tree::open_root(root).map_err(failed)?;
    let directory = upload::directory(root.as_fd()).map_err(failed)?;
    let part = format!("{TEMPORARY_PREFIX}{}", chunk.upload_id);
    let file = if chunk.offset == 0 {
        // Best effort: a sweep that fails leaves parts for the next upload.
        let _swept = upload::sweep_parts(directory.as_fd(), SystemTime::now(), STALE);
        // Ids are never reused, so anything already there was planted: removed without following it, then replaced.
        match upload::remove_part(directory.as_fd(), &part) {
            Ok(()) | Err(TreeError::NotFound) => {}
            Err(error) => return Err(failed(error)),
        }
        upload::create_part(directory.as_fd(), &part).map_err(failed)?
    } else {
        let (file, size) = upload::open_part(directory.as_fd(), &part).map_err(failed)?;
        if size != chunk.offset {
            return Err(error(
                WorkspaceErrorCode::Internal,
                "the chunk does not start where the part ends",
            ));
        }
        file
    };
    let io = |_: std::io::Error| error(WorkspaceErrorCode::Internal, "workspace I/O failed");
    file.write_all_at(&data, chunk.offset).map_err(io)?;
    let received = chunk.offset + data.len() as u64;
    if !chunk.last {
        return Ok(WorkspaceResult::Uploaded {
            received,
            file: None,
        });
    }
    file.sync_all().map_err(io)?;
    let stored = upload::link(directory.as_fd(), &part, chunk.name);
    // The link holds the bytes now; a part that cannot be removed is swept later.
    let _removed = upload::remove_part(directory.as_fd(), &part);
    let name = stored
        .map_err(failed)?
        .ok_or_else(|| error(WorkspaceErrorCode::UploadsFull, "no free name is left"))?;
    Ok(WorkspaceResult::Uploaded {
        received,
        file: Some(UploadedFile {
            path: format!("{DIRECTORY}/{name}"),
            name,
            size: received,
        }),
    })
}
