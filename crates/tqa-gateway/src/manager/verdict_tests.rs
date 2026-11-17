//! The leak verdict (the Sandboxes page design note, §3.6) from the last two sweeps, scripted: nothing unowned; a stray
//! removed in the pass; a network that fails once then goes; one that fails in two consecutive passes; a stray found
//! again after a pass tried it; bind mode and volume mode. Pure: the passes are built by hand. The row's id is claimed
//! after v18.0.0.
use super::verdict::{Finding, Kind, Pass, Tone, Verdict, verdict};
use time::{Duration, OffsetDateTime, macros::datetime};

const AT: OffsetDateTime = datetime!(2026-11-17 01:25:00 UTC);

fn pass(at: OffsetDateTime, findings: Vec<Finding>) -> Pass {
    Pass {
        at,
        running: 3,
        oldest_seconds: 25_200,
        volumes: false,
        findings,
    }
}

fn removed(kind: Kind, name: &str) -> Finding {
    Finding { kind, name: name.into(), age_seconds: 660, outcome: Ok(()) }
}

fn failed(kind: Kind, name: &str, reason: &'static str) -> Finding {
    Finding { kind, name: name.into(), age_seconds: 660, outcome: Err(reason) }
}

#[test]
fn nothing_unowned_is_green_with_the_counts_and_no_survivor() {
    let v = verdict(None, &pass(AT, vec![]));
    assert_eq!(v.tone, Tone::Green);
    assert_eq!((v.running, v.oldest_seconds), (3, 25_200));
    assert_eq!(v.unowned.containers + v.unowned.networks, 0);
    assert_eq!(v.unowned.volumes, None, "bind mode leaves volumes out");
    assert!(v.left_behind.is_empty());
    assert_eq!(v.swept_at, Some(AT));
}

#[test]
fn a_stray_removed_in_the_pass_counts_as_unowned_and_stays_green() {
    let last = pass(AT, vec![removed(Kind::Container, "0123456789ab")]);
    let v = verdict(None, &last);
    assert_eq!(v.tone, Tone::Green);
    assert_eq!(v.unowned.containers, 1);
    assert!(v.left_behind.is_empty());
}

#[test]
fn one_failed_pass_is_not_survival() {
    let previous = pass(AT - Duration::minutes(5), vec![failed(Kind::Network, "tqa-sbx-a", "engine_timeout")]);
    let last = pass(AT, vec![removed(Kind::Network, "tqa-sbx-a")]);
    let v = verdict(Some(&previous), &last);
    assert_eq!(v.tone, Tone::Green);
    assert!(v.left_behind.is_empty());
    // And the first failure alone, with no earlier pass to compare, is green too.
    assert_eq!(verdict(None, &previous).tone, Tone::Green);
}

#[test]
fn a_failure_in_two_consecutive_passes_is_amber_and_named_with_its_remove_command() {
    let previous = pass(AT - Duration::minutes(5), vec![failed(Kind::Network, "tqa-sbx-18e7c62210a67d70", "engine_timeout")]);
    let last = pass(AT, vec![failed(Kind::Network, "tqa-sbx-18e7c62210a67d70", "engine_timeout")]);
    let v = verdict(Some(&previous), &last);
    assert_eq!(v.tone, Tone::Amber);
    assert_eq!(v.left_behind.len(), 1);
    let survivor = &v.left_behind[0];
    assert_eq!(survivor.kind, Kind::Network);
    assert_eq!(survivor.name, "tqa-sbx-18e7c62210a67d70");
    assert_eq!(survivor.age_seconds, 660);
    assert_eq!(survivor.reason, "engine_timeout");
    assert_eq!(survivor.remove, "docker network rm tqa-sbx-18e7c62210a67d70");
    assert_eq!(v.left_behind_counts().networks, 1);
    assert_eq!(v.oldest_left_behind_seconds(), Some(660));
    assert_eq!(v.reason(), Some("engine_timeout"));
}

#[test]
fn a_stray_found_again_after_a_pass_tried_it_is_a_survivor() {
    let previous = pass(AT - Duration::minutes(5), vec![removed(Kind::Container, "0123456789ab")]);
    // The engine said it removed it, and it is listed again: the removal did not hold.
    let last = pass(AT, vec![failed(Kind::Container, "0123456789ab", "engine_error")]);
    let v = verdict(Some(&previous), &last);
    assert_eq!(v.tone, Tone::Amber);
    assert_eq!(v.left_behind[0].remove, "docker rm 0123456789ab");
    assert_eq!(v.left_behind[0].reason, "engine_error");
}

#[test]
fn volume_mode_counts_volumes_and_an_unowned_volume_that_survives_is_amber() {
    let mut previous = pass(AT - Duration::minutes(5), vec![failed(Kind::Volume, "tqa-ws-abc", "engine_timeout")]);
    previous.volumes = true;
    let mut last = pass(AT, vec![failed(Kind::Volume, "tqa-ws-abc", "engine_timeout")]);
    last.volumes = true;
    let v = verdict(Some(&previous), &last);
    assert_eq!(v.unowned.volumes, Some(1));
    assert_eq!(v.tone, Tone::Amber);
    assert_eq!(v.left_behind[0].remove, "docker volume rm tqa-ws-abc");
    assert_eq!(v.left_behind_counts().volumes, Some(1));
    // The readiness object names the counts the metrics gauges show, volumes present in volume mode only.
    let readiness = v.readiness();
    assert_eq!(readiness["unowned"]["volumes"], 1);
    assert_eq!(readiness["left_behind"]["volumes"], 1);
    assert_eq!(readiness["left_behind"]["oldest_seconds"], 660);
    assert_eq!(readiness["left_behind"]["reason"], "engine_timeout");
    let bind = verdict(None, &pass(AT, vec![])).readiness();
    assert!(bind["unowned"].get("volumes").is_none());
    assert!(bind["left_behind"].get("volumes").is_none());
    assert_eq!(bind["left_behind"]["reason"], serde_json::Value::Null);
}

#[test]
fn the_remove_command_is_built_from_the_engine_name_only() {
    // A name the engine never produces is still only quoted into the command, never interpreted.
    let odd = failed(Kind::Network, "tqa-sbx-x", "engine_timeout");
    let v = verdict(Some(&pass(AT - Duration::minutes(5), vec![odd.clone()])), &pass(AT, vec![odd]));
    assert_eq!(v.left_behind[0].remove, "docker network rm tqa-sbx-x");
    assert_eq!(Kind::Container.remove("abc"), "docker rm abc");
    assert_eq!(Verdict::default().tone, Tone::Green);
    assert_eq!(Verdict::default().swept_at, None);
}
