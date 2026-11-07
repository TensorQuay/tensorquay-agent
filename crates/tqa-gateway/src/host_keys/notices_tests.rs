//! GW-U-111 (ADR-0037 Amendment 1): how often the keyed-call notices go out, and who sees a key's name.
use super::{Notices, for_viewer};
use crate::host_keys::use_rule::Outcome;
use std::time::{Duration, Instant};
use tqa_types::{HostKeyOutcome, ServerFrame};
use uuid::Uuid;

// TEST: GW-U-111
#[test]
fn gw_u_111_used_once_per_run_and_key_and_a_refusal_once_a_minute() {
    let notices = Notices::default();
    let (key, other) = (Some(Uuid::now_v7()), Some(Uuid::now_v7()));
    let start = Instant::now();
    assert!(notices.due("run-1", key, Outcome::Ok, start));
    assert!(
        !notices.due("run-1", key, Outcome::Ok, start + Duration::from_hours(3)),
        "once per run and key"
    );
    assert!(notices.due("run-1", other, Outcome::Ok, start));
    assert!(notices.due("run-2", key, Outcome::Ok, start));
    assert!(notices.due("run-1", key, Outcome::WrongHost, start));
    assert!(!notices.due(
        "run-1",
        key,
        Outcome::WrongHost,
        start + Duration::from_secs(59)
    ));
    assert!(
        notices.due("run-1", key, Outcome::WrongPosition, start),
        "each outcome on its own"
    );
    assert!(notices.due(
        "run-1",
        key,
        Outcome::WrongHost,
        start + Duration::from_secs(61)
    ));
    assert!(
        notices.due("run-1", None, Outcome::Absent, start),
        "an unknown placeholder, no key"
    );
}

// TEST: GW-U-111
#[test]
fn gw_u_111_a_keys_name_reaches_only_the_runs_sender() -> Result<(), Box<dyn std::error::Error>> {
    let (sender, teammate) = (Uuid::now_v7(), Uuid::now_v7());
    let frame = ServerFrame::HostKeyRefused {
        run_id: "run-1".parse()?,
        step: Some(3),
        sender_id: sender.to_string(),
        name: Some("GITHUB_WORK".into()),
        host: "api.github.com".into(),
        port: 443,
        outcome: HostKeyOutcome::WrongHost,
    };
    let name = |frame: ServerFrame| match frame {
        ServerFrame::HostKeyRefused { name, .. } | ServerFrame::HostKeyUsed { name, .. } => name,
        _ => None,
    };
    assert_eq!(
        name(for_viewer(frame.clone(), sender)).as_deref(),
        Some("GITHUB_WORK")
    );
    assert_eq!(name(for_viewer(frame, teammate)), None);
    let used = ServerFrame::HostKeyUsed {
        run_id: "run-1".parse()?,
        step: None,
        sender_id: sender.to_string(),
        name: Some("GITHUB_WORK".into()),
        host: "api.github.com".into(),
        port: 443,
    };
    assert_eq!(name(for_viewer(used, teammate)), None);
    let other = ServerFrame::TaskTitled { title: "t".into() };
    assert_eq!(for_viewer(other.clone(), teammate), other);
    Ok(())
}
