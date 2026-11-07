use super::{Hub, start_sweep};
use crate::GatewayError;
use std::{sync::Arc, time::Duration};
use tqa_types::{ErrorCode, ServerFrame};

#[tokio::test]
async fn gw_i_007_fanout_bounds_active_conversations_and_reclaims_idle_entries()
-> Result<(), Box<dyn std::error::Error>> {
    let hub = Hub::new(1, 1);
    let first = "first".parse()?;
    let second = "second".parse()?;
    let mut subscription = hub.subscribe(&first)?;
    assert!(matches!(hub.subscribe(&second), Err(GatewayError::Busy)));
    for number in 1_u64..=2 {
        hub.node(&first)?.publish(ServerFrame::Error {
            seq: Some(number.into()),
            code: ErrorCode::Io,
            message: String::new(),
            retryable: false,
            target: None,
        });
    }
    assert!(matches!(
        subscription.receiver.recv().await,
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))
    ));
    drop(subscription);
    let _next = hub.subscribe(&second)?;
    Ok(())
}

/// An error frame numbered `seq`, as any frame a node carries.
fn frame(seq: u64) -> ServerFrame {
    ServerFrame::Error {
        seq: Some(seq.into()),
        code: ErrorCode::Io,
        message: String::new(),
        retryable: false,
        target: None,
    }
}

// TEST: GW-U-178
#[tokio::test]
async fn gw_u_178_the_sweep_drops_only_the_nodes_nobody_holds()
-> Result<(), Box<dyn std::error::Error>> {
    // The cap the soak never reaches: before, nothing below it was dropped until the task was deleted.
    let hub = Hub::new(4096, 1024);
    let (idle, streamed, appending) = ("idle".parse()?, "streamed".parse()?, "appending".parse()?);
    drop(hub.node(&idle)?);
    let mut subscription = hub.subscribe(&streamed)?;
    let in_flight = hub.node(&appending)?;
    let _permit = in_flight.append.acquire().await?;
    assert_eq!(hub.len(), 3);
    assert_eq!(hub.sweep_idle(), 1, "only the node nobody holds");
    assert_eq!(hub.len(), 2);
    // The open stream keeps its node and still gets what is published; the append's node is the same one.
    hub.node(&streamed)?.publish(frame(1));
    assert!(subscription.receiver.recv().await.is_ok());
    assert!(Arc::ptr_eq(&hub.node(&appending)?, &in_flight));
    // A swept task gets a fresh node when it is next used.
    drop(hub.node(&idle)?);
    assert_eq!(hub.len(), 3);
    Ok(())
}

// TEST: GW-U-178
#[tokio::test(start_paused = true)]
async fn gw_u_178_a_timer_sweeps_until_the_gateway_stops() -> Result<(), Box<dyn std::error::Error>>
{
    let hub = Arc::new(Hub::new(4096, 1024));
    let cancel = tokio_util::sync::CancellationToken::new();
    let tasks = tokio_util::task::TaskTracker::new();
    start_sweep(hub.clone(), Duration::from_mins(1), cancel.clone(), &tasks);
    drop(hub.node(&"first".parse()?)?);
    tokio::time::sleep(Duration::from_secs(61)).await;
    assert_eq!(hub.len(), 0, "swept at the first tick after it went idle");
    drop(hub.node(&"second".parse()?)?);
    tokio::time::sleep(Duration::from_secs(61)).await;
    assert_eq!(hub.len(), 0, "and at every tick after");
    cancel.cancel();
    tasks.close();
    tokio::time::timeout(Duration::from_secs(1), tasks.wait()).await?;
    assert!(tasks.is_empty(), "the sweep stops with the gateway");
    Ok(())
}
