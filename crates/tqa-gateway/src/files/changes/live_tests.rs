//! GW-I-048 (review of 068c): readers of one workspace share one live scan at a time.
use super::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

// TEST: GW-I-013
#[tokio::test]
async fn gw_i_048_parallel_readers_share_one_live_scan() -> Result<(), GatewayError> {
    let scans = LiveScans::new();
    let workspace = Uuid::now_v7();
    let count = Arc::new(AtomicUsize::new(0));
    let scan = |count: Arc<AtomicUsize>| async move {
        count.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(Manifest::default())
    };
    let (first, second) = tokio::join!(
        scans.get(workspace, || scan(count.clone())),
        scans.get(workspace, || scan(count.clone())),
    );
    first?;
    second?;
    assert_eq!(count.load(Ordering::SeqCst), 1);
    // Another workspace scans on its own.
    scans.get(Uuid::now_v7(), || scan(count.clone())).await?;
    assert_eq!(count.load(Ordering::SeqCst), 2);
    Ok(())
}

// TEST: GW-I-013
#[tokio::test]
async fn gw_i_048_a_later_request_scans_again() -> Result<(), GatewayError> {
    let scans = LiveScans::new();
    let workspace = Uuid::now_v7();
    let count = Arc::new(AtomicUsize::new(0));
    for _ in 0..2 {
        let count = count.clone();
        scans
            .get(workspace, || async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(Manifest::default())
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(count.load(Ordering::SeqCst), 2);
    Ok(())
}
