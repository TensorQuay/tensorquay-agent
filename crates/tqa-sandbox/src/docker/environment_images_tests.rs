//! SBX-U-012: the engine's environment images as records: newest first (ties by ID), at most `MAX_LISTED`, Docker's
//! `<none>:<none>` placeholder dropped, labels and size kept.
use super::{MAX_LISTED, records};
use bollard::models::ImageSummary;
use std::collections::HashMap;

fn summary(id: &str, created: i64, tags: &[&str]) -> ImageSummary {
    ImageSummary {
        id: id.to_owned(),
        created,
        size: 42,
        repo_tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        labels: HashMap::from([(
            "org.tensorquay.protocol-revision".to_owned(),
            "10".to_owned(),
        )]),
        ..Default::default()
    }
}

// TEST: SBX-U-012
#[test]
fn sbx_u_012_environment_images_are_newest_first_bounded_and_without_placeholder_tags() {
    let listed = records(vec![
        summary(
            "sha256:b",
            100,
            &["tqa-runtime:minimal", "tqa-runtime:base"],
        ),
        summary("sha256:c", 300, &["<none>:<none>"]),
        summary("sha256:a", 100, &["team/custom:1"]),
    ]);
    assert_eq!(
        listed
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        ["sha256:c", "sha256:a", "sha256:b"],
        "newest first, a tie by ID"
    );
    assert!(
        listed[0].tags.is_empty(),
        "the untagged image keeps no placeholder tag"
    );
    assert_eq!(listed[2].tags, ["tqa-runtime:minimal", "tqa-runtime:base"]);
    assert_eq!(
        (
            listed[1].size_bytes,
            listed[1]
                .labels
                .get("org.tensorquay.protocol-revision")
                .map(String::as_str)
        ),
        (42, Some("10"))
    );
    let times: Vec<i64> = (0..).take(MAX_LISTED + 5).collect();
    let many = records(
        times
            .iter()
            .map(|&n| summary(&format!("sha256:{n:04}"), n, &[]))
            .collect(),
    );
    assert_eq!(many.len(), MAX_LISTED);
    assert_eq!(
        Some(&many[0].created),
        times.last(),
        "the newest are the ones kept"
    );
}
