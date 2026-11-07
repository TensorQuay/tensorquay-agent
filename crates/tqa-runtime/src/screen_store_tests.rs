//! RT-I-049 (ADR-0039 section 9): the screenshot upload over the link. A capture goes as in-order `screen.upload`
//! chunks of at most 96 KiB, each written before the next; the answer decides; no answer in time drops it and a late
//! one is ignored; and a chunk an upload left behind is written before the next upload sends anything, or that one is
//! dropped too.
use super::LinkScreenStore;
use crate::{RuntimeError, link::Control, time::Time};
use base64::Engine as _;
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tqa_core::{ScreenStore, Unstored};
use tqa_testkit::{FakeClock, FakeSleeper};
use tqa_types::{MAX_UPLOAD_CHUNK_BYTES, MAX_UPLOAD_CHUNK_CHARS, ServerFrame, StoreRefusal};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Upload = tokio::task::JoinHandle<Result<(), Unstored>>;

struct Link {
    store: Arc<LinkScreenStore>,
    controls: mpsc::Receiver<Control>,
    clock: FakeClock,
}

fn link() -> Link {
    let clock = FakeClock::new();
    let time = Time {
        clock: Arc::new(clock.clone()),
        sleeper: Arc::new(FakeSleeper::new(clock.clone())),
    };
    let (sender, controls) = mpsc::channel(8);
    Link {
        store: Arc::new(LinkScreenStore::new(sender, time)),
        controls,
        clock,
    }
}

/// Lets the spawned uploads run as far as they can.
async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

fn upload(link: &Link, id: &str, png: Vec<u8>, wait: Duration) -> Upload {
    let (store, id) = (link.store.clone(), id.to_owned());
    tokio::spawn(async move { store.store(&id, &png, wait).await })
}

/// The next chunk on the link, with its write still to be confirmed: (id, chunk, last, data, its write).
type Chunk = (
    String,
    u32,
    bool,
    String,
    oneshot::Sender<Result<(), RuntimeError>>,
);

async fn next_chunk(link: &mut Link) -> Result<Chunk, Box<dyn std::error::Error>> {
    settle().await;
    let control = link.controls.try_recv()?;
    let ServerFrame::ScreenUpload {
        id,
        chunk,
        last,
        data,
    } = control.frame
    else {
        return Err("not a screen.upload".into());
    };
    Ok((
        id,
        chunk,
        last,
        data,
        control.sent.ok_or("no write to confirm")?,
    ))
}

async fn ended(upload: Upload) -> Result<Result<(), Unstored>, Box<dyn std::error::Error>> {
    settle().await;
    if !upload.is_finished() {
        return Err("the upload hasn't ended".into());
    }
    Ok(upload.await?)
}

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

// TEST: RT-I-049
#[tokio::test]
async fn rt_i_049_a_capture_goes_in_ordered_chunks_and_its_answer_decides() -> TestResult {
    let mut link = link();
    // Two whole chunks and a part one.
    let png: Vec<u8> = (0..2 * MAX_UPLOAD_CHUNK_BYTES + 100)
        .map(|index| u8::try_from(index % 251).unwrap_or(0))
        .collect();
    let running = upload(&link, A, png.clone(), Duration::from_secs(5));
    let mut sent = Vec::new();
    for expected in 0..3 {
        let (id, chunk, last, data, written) = next_chunk(&mut link).await?;
        assert_eq!((id.as_str(), chunk, last), (A, expected, expected == 2));
        assert!(data.len() <= MAX_UPLOAD_CHUNK_CHARS && data.len().is_multiple_of(4));
        // Nothing more goes until this chunk is written.
        settle().await;
        assert!(link.controls.try_recv().is_err(), "one chunk at a time");
        sent.extend(base64::engine::general_purpose::STANDARD.decode(data)?);
        written.send(Ok(())).map_err(|_| "upload gone")?;
    }
    assert_eq!(sent, png, "the chunks are the PNG, in order");
    link.store.answered(A, true, None);
    assert_eq!(ended(running).await?, Ok(()));
    Ok(())
}

// TEST: RT-I-049
#[tokio::test]
async fn rt_i_049_a_refusal_drops_the_capture_and_a_late_answer_is_ignored() -> TestResult {
    let mut link = link();
    let refused = upload(&link, A, b"PNG".to_vec(), Duration::from_secs(5));
    let (.., written) = next_chunk(&mut link).await?;
    written.send(Ok(())).map_err(|_| "upload gone")?;
    link.store.answered(A, false, Some(StoreRefusal::QuotaFull));
    assert_eq!(
        ended(refused).await?,
        Err(Unstored::Refused(StoreRefusal::QuotaFull))
    );

    // No answer within the wait: dropped, and the answer when it comes reaches nobody.
    let unanswered = upload(&link, B, b"PNG".to_vec(), Duration::from_secs(5));
    let (.., written) = next_chunk(&mut link).await?;
    written.send(Ok(())).map_err(|_| "upload gone")?;
    settle().await;
    link.clock.advance(Duration::from_secs(5))?;
    assert_eq!(ended(unanswered).await?, Err(Unstored::NoAnswer));
    link.store.answered(B, true, None);
    assert!(
        link.store
            .answers
            .lock()
            .map_err(|_| "poisoned")?
            .is_empty()
    );
    Ok(())
}

// TEST: RT-I-049
#[tokio::test]
async fn rt_i_049_a_chunk_left_behind_is_written_before_the_next_upload_starts() -> TestResult {
    let mut link = link();
    // A before capture whose chunk the link doesn't write in time (the link is down).
    let first = upload(&link, A, b"PNG-A".to_vec(), Duration::from_secs(2));
    let (_, _, _, _, left) = next_chunk(&mut link).await?;
    link.clock.advance(Duration::from_secs(2))?;
    assert_eq!(ended(first).await?, Err(Unstored::NoAnswer));

    let second = upload(&link, B, b"PNG-B".to_vec(), Duration::from_secs(5));
    settle().await;
    assert!(
        link.controls.try_recv().is_err(),
        "nothing of the next upload goes while the left chunk is unwritten"
    );
    left.send(Ok(()))
        .map_err(|_| "nobody waits for the left chunk")?;
    let (id, chunk, _, _, written) = next_chunk(&mut link).await?;
    assert_eq!((id.as_str(), chunk), (B, 0));
    written.send(Ok(())).map_err(|_| "upload gone")?;
    link.store.answered(B, true, None);
    assert_eq!(ended(second).await?, Ok(()));
    Ok(())
}

// TEST: RT-I-049
#[tokio::test]
async fn rt_i_049_a_chunk_never_written_drops_the_next_upload_unsent() -> TestResult {
    let mut link = link();
    let first = upload(&link, A, b"PNG-A".to_vec(), Duration::from_secs(2));
    let (_, _, _, _, _left) = next_chunk(&mut link).await?;
    link.clock.advance(Duration::from_secs(2))?;
    assert_eq!(ended(first).await?, Err(Unstored::NoAnswer));

    let second = upload(&link, B, b"PNG-B".to_vec(), Duration::from_secs(2));
    settle().await;
    link.clock.advance(Duration::from_secs(2))?;
    assert_eq!(ended(second).await?, Err(Unstored::NoAnswer));
    assert!(
        link.controls.try_recv().is_err(),
        "the next upload sent nothing that could cut into the left one"
    );
    Ok(())
}
