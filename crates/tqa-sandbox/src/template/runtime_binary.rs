//! The runtime binary of a template image, read without running the image (ADR-0032). The engine creates a container
//! from the image and never starts it, streams a tar archive of the one file, and the container is always removed. Only
//! the archive's single entry is read, hashed while it streams: nothing is unpacked and no path in it is used.
use std::{io::Read, time::Duration};

use async_trait::async_trait;
use futures::{StreamExt, stream::BoxStream};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::{ImageRef, SandboxError, firecracker::manifest::hex};

#[cfg(test)]
mod tests;

/// Where every first-party image keeps the runtime.
pub const RUNTIME_BINARY: &str = "/usr/local/bin/tqa-runtime";
/// The largest runtime binary read; today's is far smaller.
pub const MAX_RUNTIME_BINARY_BYTES: u64 = 256 << 20;
/// Archive bytes accepted beyond the binary: tar headers, extension records and the end blocks.
const ARCHIVE_SLACK_BYTES: u64 = 1 << 20;
/// The most bytes pulled from the engine for one archive.
const STREAM_CAP_BYTES: u64 = MAX_RUNTIME_BINARY_BYTES + ARCHIVE_SLACK_BYTES;
/// Chunks in flight between the engine's stream and the hashing thread.
pub(super) const CHUNKS_IN_FLIGHT: usize = 8;
/// How long removing the container may take, after a success or any failure.
const REMOVE_TIMEOUT: Duration = Duration::from_secs(30);

/// The engine calls reading one file out of an image needs.
#[async_trait]
pub trait ImageFiles: Send + Sync {
    /// Creates a container from `image` without starting it and without a network; returns its ID.
    async fn create_stopped(&self, image: &str) -> Result<String, SandboxError>;
    /// The engine's tar archive of `path` in the container, in chunks.
    fn archive<'a>(
        &'a self,
        container: &str,
        path: &str,
    ) -> BoxStream<'a, Result<Vec<u8>, SandboxError>>;
    /// Force-removes the container; an absent one succeeds.
    async fn remove(&self, container: &str) -> Result<(), SandboxError>;
}

/// Why the runtime binary could not be read.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RuntimeBinaryError {
    /// The image has no file at [`RUNTIME_BINARY`].
    #[error("the image has no runtime binary")]
    Missing,
    /// It is a directory, a link or another kind of entry.
    #[error("the image's runtime binary is not a regular file")]
    NotAFile,
    /// It is larger than [`MAX_RUNTIME_BINARY_BYTES`].
    #[error("the image's runtime binary is larger than 256 MiB")]
    TooLarge,
    /// The archive is not one well-formed entry.
    #[error("the engine returned a malformed archive of the runtime binary")]
    Malformed,
    /// Creating the container or reading the file took longer than its bound.
    #[error("reading the runtime binary timed out")]
    Timeout,
    /// An engine failure.
    #[error(transparent)]
    Engine(SandboxError),
}

/// The lowercase SHA-256 of the image's runtime binary. `bound` covers creating the container and reading the file;
/// removing the container has its own bound and happens after a success and after every failure.
///
/// # Errors
/// A [`RuntimeBinaryError`] naming what went wrong.
pub async fn runtime_sha256(
    files: &dyn ImageFiles,
    image: &ImageRef,
    bound: Duration,
) -> Result<String, RuntimeBinaryError> {
    let container = tokio::time::timeout(bound, files.create_stopped(&image.to_string()))
        .await
        .map_err(|_| RuntimeBinaryError::Timeout)?
        .map_err(RuntimeBinaryError::Engine)?;
    let read = tokio::time::timeout(bound, hash(files, &container, STREAM_CAP_BYTES))
        .await
        .unwrap_or(Err(RuntimeBinaryError::Timeout));
    let removed = tokio::time::timeout(REMOVE_TIMEOUT, files.remove(&container)).await;
    // A reading failure is the one to report; a removal failure matters only after a successful read.
    let digest = read?;
    removed
        .map_err(|_| RuntimeBinaryError::Timeout)?
        .map_err(RuntimeBinaryError::Engine)?;
    Ok(digest)
}

/// Streams the archive into the reader, pulling at most `cap` bytes from the engine.
async fn hash(
    files: &dyn ImageFiles,
    container: &str,
    cap: u64,
) -> Result<String, RuntimeBinaryError> {
    let (sender, receiver) = mpsc::channel::<Vec<u8>>(CHUNKS_IN_FLIGHT);
    let reader = tokio::task::spawn_blocking(move || single_entry(ChannelReader::new(receiver)));
    let mut stream = files.archive(container, RUNTIME_BINARY);
    let (mut total, mut failure, mut capped) = (0_u64, None, false);
    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(SandboxError::NotFound) => {
                failure = Some(RuntimeBinaryError::Missing);
                break;
            }
            Err(error) => {
                failure = Some(RuntimeBinaryError::Engine(error));
                break;
            }
        };
        total = total.saturating_add(chunk.len() as u64);
        if total > cap {
            capped = true;
            break;
        }
        // The reader stops early (a refusal, or the end of the archive): stop pulling from the engine.
        if sender.send(chunk).await.is_err() {
            break;
        }
    }
    drop(sender);
    let read = reader.await.map_err(|_| RuntimeBinaryError::Malformed)?;
    // Chunks still in flight when the reader reached the end count toward the cap, so the reader's own success wins;
    // a reader cut short explains itself by the stream's failure or the cap.
    match (read, failure) {
        (Ok(digest), _) => Ok(digest),
        (Err(_), Some(failure)) => Err(failure),
        (Err(_), None) if capped => Err(RuntimeBinaryError::TooLarge),
        (Err(error), None) => Err(error),
    }
}

/// Reads the archive's one entry: a regular file within the bound, followed by nothing.
fn single_entry(reader: impl Read) -> Result<String, RuntimeBinaryError> {
    let mut archive = tar::Archive::new(reader);
    let mut entries = archive
        .entries()
        .map_err(|_| RuntimeBinaryError::Malformed)?;
    let mut entry = entries
        .next()
        .ok_or(RuntimeBinaryError::Missing)?
        .map_err(|_| RuntimeBinaryError::Malformed)?;
    if !entry.header().entry_type().is_file() {
        return Err(RuntimeBinaryError::NotAFile);
    }
    if entry.size() > MAX_RUNTIME_BINARY_BYTES {
        return Err(RuntimeBinaryError::TooLarge);
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = entry
            .read(&mut buffer)
            .map_err(|_| RuntimeBinaryError::Malformed)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    drop(entry);
    if entries.next().is_some() {
        return Err(RuntimeBinaryError::Malformed);
    }
    Ok(hex(&hasher.finalize()))
}

/// The engine's chunks as a blocking reader, for the tar reader on its own thread.
pub(super) struct ChannelReader {
    receiver: mpsc::Receiver<Vec<u8>>,
    chunk: Vec<u8>,
    offset: usize,
}

impl ChannelReader {
    pub(super) fn new(receiver: mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            receiver,
            chunk: Vec::new(),
            offset: 0,
        }
    }
}

impl Read for ChannelReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        while self.offset == self.chunk.len() {
            match self.receiver.blocking_recv() {
                Some(chunk) => {
                    self.chunk = chunk;
                    self.offset = 0;
                }
                None => return Ok(0),
            }
        }
        let count = buffer.len().min(self.chunk.len() - self.offset);
        buffer[..count].copy_from_slice(&self.chunk[self.offset..self.offset + count]);
        self.offset += count;
        Ok(count)
    }
}
