use std::{sync::Arc, time::Duration};

use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{Telemetry, UPKEEP_EVERY, start_upkeep};

#[tokio::test(start_paused = true)]
async fn gw_u_177_the_histograms_are_folded_every_five_seconds_until_the_gateway_stops()
-> Result<(), crate::GatewayError> {
    let telemetry = Arc::new(Telemetry::new()?);
    for _ in 0..1000 {
        telemetry.request("GET", "/api/v1/conversations", 200, 0.002);
    }
    let (cancel, tasks) = (CancellationToken::new(), TaskTracker::new());
    start_upkeep(telemetry.clone(), cancel.clone(), &tasks);
    assert_eq!(UPKEEP_EVERY, Duration::from_secs(5));
    tokio::time::sleep(Duration::from_secs(11)).await;
    // At the start, then at 5 s and 10 s: nothing has to scrape `/metrics` for the samples to be freed.
    assert_eq!(telemetry.upkeeps(), 3);
    // The folded samples are still counted when a scrape does come.
    assert!(
        telemetry
            .render()
            .contains("tqa_http_request_duration_seconds_count{method=\"GET\",route=\"/api/v1/conversations\",status=\"200\"} 1000")
    );
    cancel.cancel();
    tasks.close();
    tasks.wait().await;
    tokio::time::sleep(UPKEEP_EVERY * 3).await;
    assert_eq!(telemetry.upkeeps(), 3);
    Ok(())
}
