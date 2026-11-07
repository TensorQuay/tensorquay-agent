//! SBX-U-008: the runtime binary is read from a created, never-started container, hashed while streaming, and the
//! container is always removed.
use std::{
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use sha2::{Digest, Sha256};

use super::{
    ImageFiles, MAX_RUNTIME_BINARY_BYTES, RUNTIME_BINARY, RuntimeBinaryError, hash, runtime_sha256,
};
use crate::{ImageRef, SandboxError, firecracker::manifest::hex};

type TestResult = Result<(), Box<dyn Error>>;

/// What the fake engine serves for the archive request.
#[derive(Clone)]
enum Served {
    Archive(Vec<u8>),
    /// A tar header claiming `size` bytes, then zeros produced lazily as they are read.
    Endless {
        size: u64,
    },
    /// A PAX extension header claiming `size` bytes of records, then zeros: the tar reader reads records whole.
    Pax {
        size: u64,
    },
    NotFound,
    /// The engine fails mid-stream with something other than a missing path.
    Broken,
    Hang,
    CreateFails,
    CreateHangs,
}

struct FakeFiles {
    served: Served,
    calls: Mutex<Vec<String>>,
    consumed: Arc<AtomicU64>,
    remove_fails: bool,
}

impl FakeFiles {
    fn new(served: Served) -> Self {
        Self {
            served,
            calls: Mutex::new(Vec::new()),
            consumed: Arc::new(AtomicU64::new(0)),
            remove_fails: false,
        }
    }
    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
    fn record(&self, call: String) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(call);
        }
    }
}

#[async_trait]
impl ImageFiles for FakeFiles {
    async fn create_stopped(&self, image: &str) -> Result<String, SandboxError> {
        self.record(format!("create {image}"));
        match self.served {
            Served::CreateFails => Err(SandboxError::ImageNotFound),
            Served::CreateHangs => std::future::pending().await,
            _ => Ok("c1".into()),
        }
    }
    fn archive<'a>(
        &'a self,
        container: &str,
        path: &str,
    ) -> BoxStream<'a, Result<Vec<u8>, SandboxError>> {
        self.record(format!("archive {container} {path}"));
        let consumed = Arc::clone(&self.consumed);
        match self.served.clone() {
            Served::Archive(bytes) => {
                let chunks: Vec<Vec<u8>> = bytes.chunks(4096).map(<[u8]>::to_vec).collect();
                counted(stream::iter(chunks).boxed(), consumed)
            }
            Served::Endless { size } | Served::Pax { size } => {
                let kind = if matches!(self.served, Served::Pax { .. }) {
                    tar::EntryType::XHeader
                } else {
                    tar::EntryType::Regular
                };
                let header = header(size, kind);
                let zeros = stream::repeat_with(|| vec![0; 1 << 20]);
                counted(stream::iter([header]).chain(zeros).boxed(), consumed)
            }
            Served::NotFound => stream::iter([Err(SandboxError::NotFound)]).boxed(),
            Served::Broken => {
                stream::iter([Err(SandboxError::Backend("engine failed".into()))]).boxed()
            }
            Served::Hang | Served::CreateFails | Served::CreateHangs => stream::pending().boxed(),
        }
    }
    async fn remove(&self, container: &str) -> Result<(), SandboxError> {
        self.record(format!("remove {container}"));
        if self.remove_fails {
            return Err(SandboxError::Backend("remove failed".into()));
        }
        Ok(())
    }
}

/// Chunks as the engine sends them, counting the bytes the reader pulls.
fn counted(
    chunks: BoxStream<'static, Vec<u8>>,
    consumed: Arc<AtomicU64>,
) -> BoxStream<'static, Result<Vec<u8>, SandboxError>> {
    chunks
        .inspect(move |chunk| {
            consumed.fetch_add(chunk.len() as u64, Ordering::SeqCst);
        })
        .map(Ok)
        .boxed()
}

fn header(size: u64, kind: tar::EntryType) -> Vec<u8> {
    let mut header = tar::Header::new_gnu();
    header.set_size(size);
    header.set_entry_type(kind);
    header.set_mode(0o755);
    let _ = header.set_path("tqa-runtime");
    header.set_cksum();
    header.as_bytes().to_vec()
}

/// A tar archive of the given entries, as the engine returns for one path.
fn archive(entries: &[(&str, tar::EntryType, &[u8])]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, kind, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_entry_type(*kind);
        header.set_mode(0o755);
        if *kind == tar::EntryType::Symlink {
            header.set_link_name("/bin/sh")?;
        }
        builder.append_data(&mut header, name, *data)?;
    }
    Ok(builder.into_inner()?)
}

fn image() -> Result<ImageRef, Box<dyn Error>> {
    Ok(ImageRef::local_id(format!("sha256:{}", "e".repeat(64)))?)
}

async fn read(files: &FakeFiles) -> Result<String, RuntimeBinaryError> {
    read_within(files, Duration::from_secs(5)).await
}

async fn read_within(files: &FakeFiles, bound: Duration) -> Result<String, RuntimeBinaryError> {
    runtime_sha256(
        files,
        &image().map_err(|_| RuntimeBinaryError::Malformed)?,
        bound,
    )
    .await
}

fn only_create_archive_remove(files: &FakeFiles) -> Result<(), Box<dyn Error>> {
    let image = image()?.to_string();
    assert_eq!(
        files.calls(),
        vec![
            format!("create {image}"),
            format!("archive c1 {RUNTIME_BINARY}"),
            "remove c1".to_owned()
        ],
        "never started, always removed"
    );
    Ok(())
}

#[tokio::test]
async fn sbx_u_008_one_regular_file_is_hashed_and_the_container_removed() -> TestResult {
    let binary = b"\x7fELF runtime".repeat(10_000);
    let files = FakeFiles::new(Served::Archive(archive(&[(
        "tqa-runtime",
        tar::EntryType::Regular,
        &binary,
    )])?));
    assert_eq!(read(&files).await?, hex(&Sha256::digest(&binary)));
    only_create_archive_remove(&files)
}

#[tokio::test]
async fn sbx_u_008_every_other_archive_shape_is_refused_and_the_container_removed() -> TestResult {
    let file = ("tqa-runtime", tar::EntryType::Regular, &b"runtime"[..]);
    let mut malformed = vec![0xff_u8; 512];
    malformed.extend([0; 1024]);
    for (served, expected) in [
        (Served::NotFound, RuntimeBinaryError::Missing),
        (Served::Archive(Vec::new()), RuntimeBinaryError::Missing),
        (
            Served::Archive(archive(&[("tqa-runtime", tar::EntryType::Directory, b"")])?),
            RuntimeBinaryError::NotAFile,
        ),
        (
            Served::Archive(archive(&[("tqa-runtime", tar::EntryType::Symlink, b"")])?),
            RuntimeBinaryError::NotAFile,
        ),
        (
            Served::Archive(archive(&[file, ("second", tar::EntryType::Regular, b"x")])?),
            RuntimeBinaryError::Malformed,
        ),
        (Served::Archive(malformed), RuntimeBinaryError::Malformed),
    ] {
        let files = FakeFiles::new(served);
        assert_eq!(read(&files).await, Err(expected.clone()), "{expected:?}");
        only_create_archive_remove(&files)?;
    }
    Ok(())
}

#[tokio::test]
async fn sbx_u_008_an_oversized_binary_is_refused_without_reading_it() -> TestResult {
    let files = FakeFiles::new(Served::Endless {
        size: MAX_RUNTIME_BINARY_BYTES + 1,
    });
    assert_eq!(read(&files).await, Err(RuntimeBinaryError::TooLarge));
    // Refused from the header: at most a few chunks were pulled, never the whole claimed size.
    assert!(files.consumed.load(Ordering::SeqCst) < 16 << 20);
    only_create_archive_remove(&files)
}

#[tokio::test]
async fn sbx_u_008_a_binary_of_exactly_the_bound_is_read_and_reading_stops_there() -> TestResult {
    // The engine would keep sending zeros after the entry; the zero blocks end the archive and nothing more is pulled.
    let files = FakeFiles::new(Served::Endless {
        size: MAX_RUNTIME_BINARY_BYTES,
    });
    let zeros = vec![0_u8; 1 << 20];
    let mut expected = Sha256::new();
    for _ in 0..(MAX_RUNTIME_BINARY_BYTES >> 20) {
        expected.update(&zeros);
    }
    // Hashing 256 MiB in a debug build takes seconds.
    assert_eq!(
        read_within(&files, Duration::from_mins(2)).await?,
        hex(&expected.finalize())
    );
    assert!(files.consumed.load(Ordering::SeqCst) <= MAX_RUNTIME_BINARY_BYTES + (16 << 20));
    only_create_archive_remove(&files)
}

#[tokio::test]
async fn sbx_u_008_a_hung_engine_times_out_and_the_container_is_still_removed() -> TestResult {
    // A real clock: paused time never auto-advances while the reader's blocking thread waits for chunks.
    let files = FakeFiles::new(Served::Hang);
    assert_eq!(
        read_within(&files, Duration::from_millis(200)).await,
        Err(RuntimeBinaryError::Timeout)
    );
    only_create_archive_remove(&files)
}

#[tokio::test]
async fn sbx_u_008_a_failed_create_leaves_nothing_to_remove() -> TestResult {
    let files = FakeFiles::new(Served::CreateFails);
    assert!(matches!(
        read(&files).await,
        Err(RuntimeBinaryError::Engine(SandboxError::ImageNotFound))
    ));
    assert_eq!(files.calls(), vec![format!("create {}", image()?)]);
    Ok(())
}

#[tokio::test]
async fn sbx_u_008_an_archive_past_the_byte_cap_is_cut_off() -> TestResult {
    // Extension records are read whole by the tar reader, so only the cap stops them; a small cap keeps the test light.
    let files = FakeFiles::new(Served::Pax { size: 4 << 20 });
    assert_eq!(
        hash(&files, "c1", 1 << 20).await,
        Err(RuntimeBinaryError::TooLarge)
    );
    assert!(files.consumed.load(Ordering::SeqCst) <= 3 << 20);
    Ok(())
}

#[tokio::test]
async fn sbx_u_008_a_truncated_archive_is_refused_not_hashed() -> TestResult {
    // The header claims 1 KiB but the engine's stream ends after 100 bytes: a partial binary is never a digest.
    let mut truncated = header(1024, tar::EntryType::Regular);
    truncated.extend([0x41; 100]);
    let files = FakeFiles::new(Served::Archive(truncated));
    assert_eq!(read(&files).await, Err(RuntimeBinaryError::Malformed));
    only_create_archive_remove(&files)
}

#[tokio::test]
async fn sbx_u_008_engine_failures_are_reported_and_the_container_removed() -> TestResult {
    let broken = FakeFiles::new(Served::Broken);
    assert!(matches!(
        read(&broken).await,
        Err(RuntimeBinaryError::Engine(SandboxError::Backend(_)))
    ));
    only_create_archive_remove(&broken)?;
    // A container that would not go away after a good read is reported, not ignored.
    let mut leftover = FakeFiles::new(Served::Archive(archive(&[(
        "tqa-runtime",
        tar::EntryType::Regular,
        b"runtime",
    )])?));
    leftover.remove_fails = true;
    assert!(matches!(
        read(&leftover).await,
        Err(RuntimeBinaryError::Engine(SandboxError::Backend(_)))
    ));
    only_create_archive_remove(&leftover)
}

#[tokio::test]
async fn sbx_u_008_a_create_that_hangs_times_out_with_nothing_to_remove() -> TestResult {
    let files = FakeFiles::new(Served::CreateHangs);
    assert_eq!(
        read_within(&files, Duration::from_millis(200)).await,
        Err(RuntimeBinaryError::Timeout)
    );
    assert_eq!(files.calls(), vec![format!("create {}", image()?)]);
    Ok(())
}

#[test]
fn sbx_u_008_the_stream_cap_is_the_binary_bound_plus_one_mib_of_archive_overhead() {
    assert_eq!(super::STREAM_CAP_BYTES, (256 << 20) + (1 << 20));
}

#[tokio::test]
async fn sbx_u_008_an_archive_of_exactly_the_cap_is_read() -> TestResult {
    let bytes = archive(&[("tqa-runtime", tar::EntryType::Regular, b"runtime")])?;
    let cap = u64::try_from(bytes.len())?;
    let files = FakeFiles::new(Served::Archive(bytes.clone()));
    assert_eq!(
        hash(&files, "c1", cap).await?,
        hex(&Sha256::digest(b"runtime"))
    );
    Ok(())
}
