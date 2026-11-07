//! SBX-U-008 (engine side, without an engine): check containers are recognisable, unique, isolated and never started.
//! SBX-D-009 (without an engine): which listed leftovers a sweep removes.
use std::time::{Duration, SystemTime};

use bollard::models::ContainerSummary;

use super::{CHECK_LABEL, CHECK_PREFIX, MAX_SWEPT, check_body, check_name, due};

#[test]
fn sbx_u_008_check_containers_are_named_labelled_and_without_a_network() {
    let (first, second) = (check_name(), check_name());
    assert!(first.starts_with(CHECK_PREFIX) && second.starts_with(CHECK_PREFIX));
    assert_ne!(first, second);
    let body = check_body("sha256:abc", "tqa-unit");
    assert_eq!(body.image.as_deref(), Some("sha256:abc"));
    assert_eq!(body.network_disabled, Some(true));
    assert_eq!(
        body.host_config
            .and_then(|host| host.network_mode)
            .as_deref(),
        Some("none")
    );
    let labels = body.labels.unwrap_or_default();
    assert_eq!(labels.get(CHECK_LABEL).map(String::as_str), Some("1"));
    assert_eq!(
        labels.get("tqa.instance").map(String::as_str),
        Some("tqa-unit")
    );
    // Nothing overrides what would run: the container is never started, so the image's own command stays inert.
    assert!(body.entrypoint.is_none() && body.cmd.is_none());
}

fn container(id: Option<&str>, created: Option<i64>) -> ContainerSummary {
    ContainerSummary {
        id: id.map(str::to_owned),
        created,
        ..Default::default()
    }
}

#[test]
fn sbx_d_009_a_sweep_takes_only_containers_at_least_the_bound_old() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
    let bound = Duration::from_mins(2);
    let listed = [
        container(Some("old"), Some(100)),
        container(Some("at-the-bound"), Some(880)),
        container(Some("fresh"), Some(881)),
        container(None, Some(100)),
        container(Some("no-time"), None),
        container(Some("before-the-epoch"), Some(-1)),
    ];
    assert_eq!(due(&listed, now, bound), ["old", "at-the-bound"]);
    // A bound longer than the clock reaches takes nothing.
    assert!(due(&listed, now, Duration::MAX).is_empty());
    // One sweep is bounded; the next start-up takes the rest.
    let many: Vec<_> = (0..=MAX_SWEPT)
        .map(|index| container(Some(&format!("c{index}")), Some(1)))
        .collect();
    assert_eq!(due(&many, now, bound).len(), MAX_SWEPT);
}
