//! GW-U-137 (GW-I-129's start-up failure under load): start-up's one hash waits within its own long bound, while every other
//! password call keeps `password_seconds`. GW-U-141: that bound holds the wait for a worker and the work together, and
//! the queue is bounded: past its places a call is refused at once.
use super::Passwords;
use crate::{GatewayConfig, GatewayError};
use std::time::Duration;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn passwords(cancel: &CancellationToken) -> Result<Passwords, GatewayError> {
    bounded(cancel, 1)
}

/// One password worker, and `seconds` for each password operation.
fn bounded(cancel: &CancellationToken, seconds: u64) -> Result<Passwords, GatewayError> {
    let config = GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:0'\npublic_origin='https://gateway.example'\nmodels_path='models.toml'\n\
         [storage]\nroot='/gateway-storage'\n[limits]\nio_timeout=1\npassword_workers=1\npassword_seconds={seconds}"
    ))?;
    Ok(Passwords::new(&config, TaskTracker::new(), cancel.clone()))
}

// TEST: GW-U-137
#[tokio::test]
async fn gw_u_137_start_ups_hash_outwaits_the_io_bound_and_shutdown_stops_it() -> TestResult {
    let cancel = CancellationToken::new();
    let passwords = passwords(&cancel)?;
    // A loaded host: the only password worker is busy for longer than the I/O bound.
    let busy = passwords.admission.clone().acquire_owned().await?;
    assert_eq!(
        passwords.hash("synthetic passphrase".into()).await,
        Err(GatewayError::Timeout),
        "an ordinary hash keeps its own bound"
    );
    let at_start = tokio::spawn({
        let passwords = passwords.clone();
        async move { passwords.hash_at_start("synthetic passphrase".into()).await }
    });
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        !at_start.is_finished(),
        "start-up's hash still waits past it"
    );
    drop(busy);
    let hash = at_start.await??;
    assert!(hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
    // Shutdown stops the wait.
    let busy = passwords.admission.clone().acquire_owned().await?;
    let stopped = tokio::spawn({
        let passwords = passwords.clone();
        async move { passwords.hash_at_start("synthetic passphrase".into()).await }
    });
    cancel.cancel();
    assert_eq!(stopped.await?, Err(GatewayError::Cancelled));
    drop(busy);
    Ok(())
}

// TEST: GW-U-141
#[test]
fn gw_u_141_password_seconds_defaults_to_30_within_1_to_300() -> TestResult {
    let base = "listen='127.0.0.1:0'\npublic_origin='https://gateway.example'\nmodels_path='models.toml'\n\
                [storage]\nroot='/gateway-storage'\n";
    let config = GatewayConfig::from_toml(base)?;
    assert_eq!(config.password_bound(), Duration::from_secs(30));
    for (seconds, valid) in [(0, false), (1, true), (300, true), (301, false)] {
        assert_eq!(
            GatewayConfig::from_toml(&format!("{base}[limits]\npassword_seconds={seconds}"))
                .is_ok(),
            valid,
            "{seconds}"
        );
    }
    Ok(())
}

// TEST: GW-U-141
#[tokio::test]
async fn gw_u_141_past_its_waiting_places_a_call_is_refused_at_once() -> TestResult {
    let cancel = CancellationToken::new();
    let passwords = bounded(&cancel, 60)?;
    // The only worker busy: one worker's nine places (itself and eight waiting) fill, and the next call is refused.
    let busy = passwords.admission.clone().acquire_owned().await?;
    let waiting: Vec<_> = (0..9)
        .map(|_| {
            let passwords = passwords.clone();
            tokio::spawn(async move { passwords.hash("synthetic passphrase".into()).await })
        })
        .collect();
    while passwords.callers.load(std::sync::atomic::Ordering::Acquire) < 9 {
        tokio::task::yield_now().await;
    }
    let refused = tokio::time::timeout(
        Duration::from_millis(100),
        passwords.verify(
            "synthetic passphrase".into(),
            "$argon2id$v=19$m=19456,t=2,p=1$x".into(),
        ),
    )
    .await;
    assert_eq!(
        refused,
        Ok(Err(GatewayError::Busy)),
        "refused at once, not after its bound"
    );
    // The waiting calls all finish once the worker is free, and their places are given back.
    drop(busy);
    for call in waiting {
        assert!(call.await??.starts_with("$argon2id$"));
    }
    assert_eq!(
        passwords.callers.load(std::sync::atomic::Ordering::Acquire),
        0
    );
    assert!(
        passwords.hash("synthetic passphrase".into()).await.is_ok(),
        "a place again"
    );
    Ok(())
}

// TEST: GW-U-141
#[tokio::test]
async fn gw_u_141_the_wait_and_the_work_share_one_bound() -> TestResult {
    let cancel = CancellationToken::new();
    let passwords = bounded(&cancel, 1)?;
    // The worker comes free with about 10 ms of the 1 s bound left: too little for a hash, which the work must not
    // get a second bound for.
    let busy = passwords.admission.clone().acquire_owned().await?;
    let call = tokio::spawn({
        let passwords = passwords.clone();
        async move { passwords.hash("synthetic passphrase".into()).await }
    });
    tokio::time::sleep(Duration::from_millis(990)).await;
    drop(busy);
    assert_eq!(call.await?, Err(GatewayError::Timeout));
    Ok(())
}
