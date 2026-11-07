//! GW-U-089: built-in environments from the engine's images. A built-in is the image its canonical tag names
//! (tqa-runtime:<name>) when that image's environment label agrees; an image that only inherits the label (built FROM
//! Workstation) never replaces it. Every other image is classified with plain reasons for the image list.
use super::{EngineImage, OwnSize, Runtime, built_ins, classify, own_size};
use crate::config::BuiltinImages;
use std::collections::BTreeMap;
use tqa_types::{Capability, MIN_RUNTIME_REVISION, RUNTIME_REVISION_LABEL};

fn image(id: &str, tags: &[&str], created: i64, labels: &[(&str, &str)]) -> EngineImage {
    EngineImage {
        id: format!("sha256:{id:0>64}"),
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        created,
        size_bytes: 1 << 30,
        labels: labels
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn current() -> String {
    MIN_RUNTIME_REVISION.to_string()
}

fn environment<'a>(
    name: &'a str,
    display: &'a str,
    capabilities: &'a str,
    revision: &'a str,
) -> Vec<(&'a str, &'a str)> {
    vec![
        (RUNTIME_REVISION_LABEL, revision),
        ("org.tensorquay.environment", name),
        ("org.tensorquay.environment.name", display),
        ("org.tensorquay.environment.capabilities", capabilities),
    ]
}

// TEST: GW-U-089
#[test]
fn gw_u_089_a_built_in_is_the_image_its_canonical_tag_names_when_its_label_agrees() {
    let revision = current();
    let images = [
        image(
            "a1",
            &["tqa-runtime:minimal", "tqa-runtime:base"],
            100,
            &environment("minimal", "Minimal", "shell", &revision),
        ),
        image(
            "b2",
            &["tqa-runtime:workstation"],
            200,
            &environment(
                "workstation",
                "Workstation",
                "shell,python,node,build,browser,data",
                &revision,
            ),
        ),
    ];
    let found = built_ins(&images, &BuiltinImages::default());
    assert_eq!(
        found
            .iter()
            .map(|built| (
                built.name,
                built.display_name.as_str(),
                built.image.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            ("minimal", "Minimal", images[0].id.as_str()),
            ("workstation", "Workstation", images[1].id.as_str())
        ]
    );
    assert_eq!(
        found[1].capabilities,
        [
            Capability::Shell,
            Capability::Python,
            Capability::Node,
            Capability::Build,
            Capability::Browser,
            Capability::Data
        ]
    );
    assert_eq!(found[1].created, 200);
}

// TEST: GW-U-089
#[test]
fn gw_u_089_an_image_built_from_workstation_inherits_its_label_but_never_replaces_it() {
    // F10: a person's customised environment, built later FROM Workstation, carries the same labels.
    let revision = current();
    let labels = environment(
        "workstation",
        "Workstation",
        "shell,python,node,build,browser,data",
        &revision,
    );
    let canonical = image("b2", &["tqa-runtime:workstation"], 200, &labels);
    let derived = image("c3", &["team/data-science:1"], 900, &labels);
    let images = [canonical.clone(), derived.clone()];
    let found = built_ins(&images, &BuiltinImages::default());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].image, canonical.id);
    let listed = classify(&derived, &BuiltinImages::default());
    assert_eq!(listed.based_on.as_deref(), Some("Workstation"));
    assert!(
        listed.usable,
        "a derived image can still be picked for a new environment"
    );
    assert_eq!(
        classify(&canonical, &BuiltinImages::default()).based_on,
        None
    );
}

// TEST: GW-U-089
#[test]
fn gw_u_089_a_canonical_tag_on_an_image_labelled_as_another_environment_makes_no_built_in() {
    let images = [image(
        "d4",
        &["tqa-runtime:workstation"],
        100,
        &environment("minimal", "Minimal", "shell", &current()),
    )];
    assert!(
        built_ins(&images, &BuiltinImages::default())
            .iter()
            .all(|built| built.name != "workstation")
    );
}

// TEST: GW-U-089
#[test]
fn gw_u_089_unusable_images_are_listed_with_plain_reasons_and_are_never_built_ins() {
    let too_old = (MIN_RUNTIME_REVISION - 1).to_string();
    let cases = [
        (
            image(
                "e5",
                &["tqa-runtime:workstation"],
                1,
                &environment("workstation", "Workstation", "shell,gpu", &current()),
            ),
            Runtime::Current,
            "its capability label names an unknown capability: gpu",
        ),
        (
            image(
                "f6",
                &["tqa-runtime:workstation"],
                1,
                &environment("workstation", "Workstation", "", &current()),
            ),
            Runtime::Current,
            "its capability label names no capability",
        ),
        (
            image(
                "a7",
                &["tqa-runtime:minimal"],
                1,
                &environment("minimal", "Minimal", "shell", &too_old),
            ),
            Runtime::TooOld,
            "too old for this version: rebuild it",
        ),
        (
            image("b8", &["other:1"], 1, &[]),
            Runtime::None,
            "there is no TensorQuay runtime in it",
        ),
    ];
    for (listed, runtime, reason) in &cases {
        let classified = classify(listed, &BuiltinImages::default());
        assert_eq!(classified.runtime, *runtime, "{reason}");
        assert!(!classified.usable, "{reason}");
        assert_eq!(classified.reason.as_deref(), Some(*reason));
        assert!(
            built_ins(std::slice::from_ref(listed), &BuiltinImages::default()).is_empty(),
            "{reason}"
        );
    }
    assert!(!classify(&cases[0].0, &BuiltinImages::default()).capabilities_valid);
}

// TEST: GW-U-091
#[test]
fn gw_u_091_a_recipe_image_is_based_on_its_built_in_and_never_one() {
    // deploy/build-environment.sh labels its image with the recipe's own name and the built-in it starts from.
    let revision = current();
    let mut labels = environment("data-r", "R and LaTeX", "shell,python", &revision);
    labels.push(("org.tensorquay.environment.base", "workstation"));
    let recipe = image("c9", &["tqa-env:data-r"], 300, &labels);
    let listed = classify(&recipe, &BuiltinImages::default());
    assert_eq!(listed.based_on.as_deref(), Some("Workstation"));
    assert_eq!(listed.environment.as_deref(), Some("data-r"));
    assert!(listed.usable);
    assert!(built_ins(std::slice::from_ref(&recipe), &BuiltinImages::default()).is_empty());
    // A base label that names no built-in says nothing.
    let mut odd = environment("data-r", "R and LaTeX", "shell", &revision);
    odd.push(("org.tensorquay.environment.base", "custom"));
    assert_eq!(
        classify(&image("d1", &[], 1, &odd), &BuiltinImages::default()).based_on,
        None
    );
}

fn workstation(id: &str, tags: &[&str], created: i64) -> EngineImage {
    image(
        id,
        tags,
        created,
        &environment(
            "workstation",
            "Workstation",
            "shell,python,node,build,browser,data",
            &current(),
        ),
    )
}

fn pinned(name: &str, image: &EngineImage) -> Result<BuiltinImages, crate::GatewayError> {
    BuiltinImages::new([(name.to_owned(), image.id.clone())].into_iter().collect())
}

// TEST: GW-U-120
#[test]
fn gw_u_120_a_pinned_built_in_is_its_image_by_id_never_the_tag() -> Result<(), crate::GatewayError>
{
    // The shared engine's tag names another build (a candidate's); the stack's own image carries no tag at all.
    let tagged = workstation("c3", &["tqa-runtime:workstation"], 300);
    let under_test = workstation("d4", &[], 200);
    let images = [tagged.clone(), under_test.clone()];
    let found = built_ins(&images, &pinned("workstation", &under_test)?);
    assert_eq!(
        found
            .iter()
            .map(|built| (built.name, built.image.as_str()))
            .collect::<Vec<_>>(),
        [("workstation", under_test.id.as_str())]
    );
    // The image list agrees: the pinned image is the built-in, the tagged one only carries its label.
    let pins = pinned("workstation", &under_test)?;
    assert_eq!(classify(&under_test, &pins).based_on, None);
    assert_eq!(
        classify(&tagged, &pins).based_on.as_deref(),
        Some("Workstation")
    );
    Ok(())
}

// TEST: GW-U-120
#[test]
fn gw_u_120_a_pinned_image_missing_from_the_engine_is_no_built_in_and_the_tag_does_not_stand_in()
-> Result<(), crate::GatewayError> {
    let tagged = workstation("c3", &["tqa-runtime:workstation"], 300);
    let absent = workstation("e5", &[], 200);
    let found = built_ins(
        std::slice::from_ref(&tagged),
        &pinned("workstation", &absent)?,
    );
    assert!(
        found.is_empty(),
        "{:?}",
        found.iter().map(|built| &built.image).collect::<Vec<_>>()
    );
    Ok(())
}

// TEST: GW-U-120
#[test]
fn gw_u_120_a_pinned_image_whose_label_names_another_environment_is_no_built_in()
-> Result<(), crate::GatewayError> {
    let minimal = image(
        "f6",
        &[],
        100,
        &environment("minimal", "Minimal", "shell", &current()),
    );
    assert!(
        built_ins(
            std::slice::from_ref(&minimal),
            &pinned("workstation", &minimal)?
        )
        .is_empty()
    );
    Ok(())
}

// TEST: GW-U-120
#[test]
fn gw_u_120_a_pinned_image_missing_from_the_engine_is_named_so_registration_can_say_why()
-> Result<(), crate::GatewayError> {
    let present = workstation("d4", &[], 200);
    let absent = workstation("e5", &[], 200);
    let images = [present.clone()];
    assert!(super::missing_pins(&images, &pinned("workstation", &present)?).is_empty());
    assert_eq!(
        super::missing_pins(&images, &pinned("workstation", &absent)?),
        ["workstation"]
    );
    assert!(
        super::missing_pins(&images, &BuiltinImages::default()).is_empty(),
        "nothing pinned, nothing missing"
    );
    Ok(())
}

// TEST: GW-U-197
#[test]
fn gw_u_197_desktop_has_its_own_size_and_the_other_built_ins_take_the_defaults() {
    // ADR-0039 section 10: a display and a headed browser need more than the defaults. One source for the registration
    // and the image list, so a Desktop added again from its card gets the size the registration gives it.
    let desktop = own_size("desktop");
    assert_eq!(
        desktop,
        Some(OwnSize {
            cpu_millicores: 2000,
            memory_bytes: 4 << 30,
            pids: 1024,
        })
    );
    assert_eq!(
        desktop.map(OwnSize::listed),
        Some(serde_json::json!({"cpus": 2.0, "memory_mib": 4096, "pids": 1024}))
    );
    for name in ["minimal", "workstation", "data"] {
        assert_eq!(own_size(name), None, "{name}");
    }
}
