use super::{RelayConfig, rate::Rate};
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn gw_i_039_relay_configuration_rejects_unbounded_or_unknown_settings() -> TestResult {
    RelayConfig::default().validate()?;
    for source in [
        "capacity=0",
        "refill_per_second=nan",
        "refill_per_second=0.0",
        "users=0",
        "body_bytes=16777217",
        "sse_event_bytes=0",
        "response_bytes=0",
        "header_timeout=0",
        "idle_timeout=0",
        "response_timeout=0",
        "unknown=true",
    ] {
        let config = toml::from_str::<RelayConfig>(source);
        assert!(config.is_err() || config.is_ok_and(|config| config.validate().is_err()));
    }
    Ok(())
}
#[test]
fn gw_i_039_token_buckets_refill_by_time_and_keep_owners_independent() -> TestResult {
    let config = RelayConfig {
        capacity: Some(2),
        refill_per_second: Some(2.0),
        users: 2,
        ..RelayConfig::default()
    };
    let rate = Rate::new(&config);
    let first = uuid::Uuid::now_v7();
    let second = uuid::Uuid::now_v7();
    let now = Instant::now();
    rate.admit(first, now, 1)?;
    rate.admit(first, now, 1)?;
    assert!(rate.admit(first, now, 1).is_err());
    rate.admit(second, now, 1)?;
    rate.admit(first, now + Duration::from_millis(500), 1)?;
    assert!(
        rate.admit(first, now + Duration::from_millis(500), 1)
            .is_err()
    );
    rate.admit(uuid::Uuid::now_v7(), now + Duration::from_secs(2), 1)?;
    Ok(())
}
#[test]
fn gw_i_039_active_rate_buckets_never_grow_past_the_user_bound() -> TestResult {
    let rate = Rate::new(&RelayConfig {
        capacity: Some(1),
        users: 1,
        ..RelayConfig::default()
    });
    let now = Instant::now();
    rate.admit(uuid::Uuid::now_v7(), now, 1)?;
    assert!(rate.admit(uuid::Uuid::now_v7(), now, 1).is_err());
    Ok(())
}

const GATEWAY: &str = "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\n\
    models_path='/etc/tqa/models.toml'\n[storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\n\
    [sandbox]\ngateway_url='http://tqa-gateway:8080'\n";

fn relay_for(sections: &str) -> Result<RelayConfig, Box<dyn std::error::Error>> {
    Ok(crate::GatewayConfig::from_toml(&format!("{GATEWAY}{sections}"))?.relay)
}

/// Requests one user's bucket admits at `now`, then after one more second, with the file's live runs per user.
fn admitted(config: &RelayConfig) -> (usize, usize) {
    let rate = Rate::new(config);
    let live_runs = config.live_runs;
    let user = uuid::Uuid::now_v7();
    let now = Instant::now();
    let burst = (0..200_000)
        .take_while(|_| rate.admit(user, now, live_runs).is_ok())
        .count();
    let later = now + Duration::from_secs(1);
    let refill = (0..200_000)
        .take_while(|_| rate.admit(user, later, live_runs).is_ok())
        .count();
    (burst, refill)
}

#[test]
fn gw_u_032_the_per_user_relay_rate_scales_with_the_users_live_runs() -> TestResult {
    // One live run per user keeps the original allowance: a burst of 60, then 1 a second.
    assert_eq!(admitted(&relay_for("max_live=1\n")?), (60, 1));
    // Per-user admission defaults to max_live: 16 live runs get 16 times the allowance.
    assert_eq!(admitted(&relay_for("max_live=16\n")?), (960, 16));
    assert_eq!(
        admitted(&relay_for("max_live=16\n[admission]\nper_user=4\n")?),
        (240, 4)
    );
    // Explicit settings win over the derived ones.
    assert_eq!(
        admitted(&relay_for(
            "max_live=16\n[relay]\ncapacity=5\nrefill_per_second=2.0\n"
        )?),
        (5, 2)
    );
    assert_eq!(
        admitted(&relay_for("max_live=16\n[relay]\ncapacity=20\n")?),
        (20, 16)
    );
    // A derived burst stays within the configured bound.
    assert_eq!(admitted(&relay_for("max_live=4096\n")?).0, 100_000);
    Ok(())
}

#[test]
fn gw_u_032_every_relay_bound_is_enforced_at_both_edges() -> TestResult {
    // (field, lowest accepted, highest accepted, just below, just above); every bound is checked on its own, so no
    // single condition of the validator can be dropped unnoticed.
    let bounds = [
        ("capacity", "1", "100000", "0", "100001"),
        ("refill_per_second", "0.001", "10000.0", "0.0009", "10000.1"),
        ("users", "1", "100000", "0", "100001"),
        ("body_bytes", "1024", "16777216", "1023", "16777217"),
        ("sse_event_bytes", "128", "4194304", "127", "4194305"),
        ("response_bytes", "1024", "268435456", "1023", "268435457"),
        ("header_timeout", "1", "300", "0", "301"),
        ("idle_timeout", "1", "300", "0", "301"),
        ("response_timeout", "1", "3600", "0", "3601"),
        ("queue_wait", "1", "120", "0", "121"),
        ("queue_max", "1", "10000", "0", "10001"),
    ];
    for (field, low, high, below, above) in bounds {
        for value in [low, high] {
            let config = toml::from_str::<RelayConfig>(&format!("{field}={value}"))?;
            assert!(
                config.validate().is_ok(),
                "{field}={value} must be accepted"
            );
        }
        for value in [below, above] {
            let config = toml::from_str::<RelayConfig>(&format!("{field}={value}"))?;
            assert!(
                config.validate().is_err(),
                "{field}={value} must be refused"
            );
        }
    }
    // A refill that is not a finite number is refused whatever its range check would say.
    for value in ["nan", "inf"] {
        let config = toml::from_str::<RelayConfig>(&format!("refill_per_second={value}"))?;
        assert!(config.validate().is_err(), "refill_per_second={value}");
    }
    Ok(())
}

/// Requests `rate` admits for one new user at one instant, with `live_runs` as the effective `admission.per_user`.
fn burst(rate: &Rate, live_runs: u32) -> usize {
    let user = uuid::Uuid::now_v7();
    let now = Instant::now();
    (0..1000)
        .take_while(|_| rate.admit(user, now, live_runs).is_ok())
        .count()
}

#[test]
fn gw_i_062_the_relay_rate_follows_the_effective_per_user_limit() {
    // Unset capacity and refill follow the runs a user may have live, now the effective (overridable) per_user.
    let rate = Rate::new(&RelayConfig::default());
    assert_eq!(burst(&rate, 1), 60);
    assert_eq!(
        burst(&rate, 2),
        120,
        "an override raising per_user raises the burst for the next request"
    );
    // An explicit capacity still wins over the derived one.
    let fixed = Rate::new(&RelayConfig {
        capacity: Some(5),
        ..RelayConfig::default()
    });
    assert_eq!(burst(&fixed, 16), 5);
}
