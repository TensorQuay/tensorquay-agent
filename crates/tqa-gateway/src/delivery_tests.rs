use super::{ReplayCursor, commit_then_publish};
use crate::GatewayError;
use futures::{FutureExt, executor::block_on};
use proptest::prelude::*;
use std::{cell::RefCell, collections::VecDeque, task::Poll};
use tqa_types::Seq;

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, rng_seed: proptest::test_runner::RngSeed::Fixed(17), ..ProptestConfig::default() })]
    #[test]
    fn gw_i_007_replay_handoff_preserves_every_sequence(
        interleavings in prop::collection::vec(any::<bool>(), 1..128),
        resume in 0_usize..128,
    ) {
        let count = interleavings.len();
        let after = resume.min(count);
        let store = RefCell::new((1..=after).collect::<Vec<_>>());
        let live = RefCell::new(VecDeque::new());
        let watermark = after + interleavings.iter().skip(after).filter(|value| **value).count();
        let mut cursor = ReplayCursor::new(Seq::from(after as u64), Seq::from(watermark as u64))?;
        let mut received = Vec::new();
        let mut replayed = after;
        // Subscription precedes the snapshot. Reads and commits then interleave deterministically.
        for (index, delayed) in interleavings.iter().enumerate().skip(after) {
            let seq = index + 1;
            let mut waiting = *delayed;
            let write = futures::future::poll_fn(|context| {
                if waiting {
                    waiting = false;
                    context.waker().wake_by_ref();
                    return Poll::Pending;
                }
                store.borrow_mut().push(seq);
                Poll::Ready(Ok(()))
            });
            let publish = || {
                // A browser is allowed to reconnect as soon as a frame is published.
                assert!(store.borrow().contains(&seq), "published before durable storage");
                live.borrow_mut().push_back(seq);
            };
            prop_assert!(block_on(commit_then_publish(write, publish)).is_ok());
            if seq >= watermark && *delayed && replayed < watermark {
                replayed += 1;
                prop_assert_eq!(store.borrow().get(replayed - 1).copied(), Some(replayed));
                cursor.replay(Seq::from(replayed as u64))?;
                received.push(replayed);
            }
            if replayed == watermark && !*delayed { drain_live(&mut cursor, &live, &mut received)?; }
        }
        for seq in (replayed + 1)..=watermark {
            prop_assert_eq!(store.borrow().get(seq - 1).copied(), Some(seq));
            cursor.replay(Seq::from(seq as u64))?;
            received.push(seq);
        }
        if cursor.last() <= Seq::from(watermark as u64) { cursor.finish_replay()?; }
        drain_live(&mut cursor, &live, &mut received)?;
        prop_assert_eq!(received, ((after + 1)..=count).collect::<Vec<_>>());
    }
}

fn drain_live(
    cursor: &mut ReplayCursor,
    live: &RefCell<VecDeque<usize>>,
    received: &mut Vec<usize>,
) -> Result<(), GatewayError> {
    while let Some(seq) = live.borrow_mut().pop_front() {
        if cursor.live(Seq::from(seq as u64))? {
            received.push(seq);
        }
    }
    Ok(())
}

#[test]
fn gw_i_005_failed_or_unfinished_storage_never_publishes() {
    let published = RefCell::new(false);
    let pending = commit_then_publish(futures::future::pending(), || {
        *published.borrow_mut() = true;
    });
    assert!(pending.now_or_never().is_none());
    assert!(!*published.borrow());
    let failed = block_on(commit_then_publish(
        async { Err(GatewayError::StoreUnavailable) },
        || *published.borrow_mut() = true,
    ));
    assert_eq!(failed, Err(GatewayError::StoreUnavailable));
    assert!(!*published.borrow());
}

#[test]
fn proto_u_004_replay_and_live_reject_gaps_and_incomplete_snapshots() -> Result<(), GatewayError> {
    assert!(ReplayCursor::new(2.into(), 1.into()).is_err());
    let mut cursor = ReplayCursor::new(0.into(), 2.into())?;
    assert!(cursor.live(1.into()).is_err());
    assert!(cursor.finish_replay().is_err());
    assert!(cursor.replay(2.into()).is_err());
    cursor.replay(1.into())?;
    cursor.replay(2.into())?;
    assert!(cursor.replay(3.into()).is_err());
    cursor.finish_replay()?;
    assert!(!cursor.live(1.into())?);
    assert!(!cursor.live(2.into())?);
    assert!(cursor.live(4.into()).is_err());
    assert!(cursor.live(3.into())?);
    assert!(cursor.live(3.into()).is_err());
    Ok(())
}
