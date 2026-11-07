//! GW-I-063 (configuration): a `[skills] root` is refused as written when it is malformed or overlaps storage.
use super::{SkillsInput, skills};
use std::path::{Path, PathBuf};

fn input(root: &str) -> SkillsInput {
    SkillsInput {
        root: Some(PathBuf::from(root)),
    }
}

#[test]
fn gw_i_063_a_malformed_or_overlapping_root_is_refused() {
    let storage = Path::new("/var/lib/tqa");
    for root in [
        "skills",
        "/",
        "/srv/../var/lib/tqa",
        "/var/lib/tqa",
        "/var/lib/tqa/skills",
        "/var/lib",
    ] {
        assert!(skills(input(root), storage).is_err(), "{root} accepted");
    }
}

#[test]
fn gw_i_063_a_separate_root_is_kept_and_none_means_no_install_tier() {
    let storage = Path::new("/var/lib/tqa");
    let configured = skills(input("/srv/tqa-skills"), storage)
        .ok()
        .and_then(|s| s.root);
    assert_eq!(configured, Some(PathBuf::from("/srv/tqa-skills")));
    let absent = skills(SkillsInput::default(), storage)
        .ok()
        .and_then(|s| s.root);
    assert_eq!(absent, None);
    assert!(
        skills(input("/var/lib/tqa-skills"), storage).is_ok(),
        "a sibling is not an overlap"
    );
}
