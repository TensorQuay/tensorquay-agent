//! SBX-U-007, SBX-U-017: the template image checks that need no engine (ADR-0032).
use std::{collections::BTreeSet, error::Error};

use bollard::models::ImageInspect;
use serde_json::{Value, json};
use tqa_types::{MIN_RUNTIME_REVISION, PROTOCOL_REVISION, RUNTIME_REVISION_LABEL};

use super::{BaseImage, Bases, MAX_BASE_IMAGES, TemplateRefusal, check_image};

type TestResult = Result<(), Box<dyn Error>>;

fn layers(prefix: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("sha256:{prefix}{index:063x}"))
        .collect()
}

/// Minimal: layers m0..m2. Workstation derives from it: Minimal's layers plus w0..w1.
fn bases() -> Result<Bases, Box<dyn Error>> {
    let minimal = layers("a", 3);
    let workstation = [minimal.clone(), layers("b", 2)].concat();
    Ok(Bases::new(vec![
        BaseImage::new(
            "minimal",
            minimal,
            vec!["/usr/local/bin/tqa-runtime".into()],
        ),
        BaseImage::new(
            "workstation",
            workstation,
            vec!["/usr/local/bin/tqa-runtime".into()],
        ),
    ])?)
}

/// An image that passes every check on top of `base_layers`; tests change one field at a time.
fn image(image_layers: &[String]) -> Value {
    json!({
        "Id": format!("sha256:{}", "e".repeat(64)),
        "RootFS": {"Type": "layers", "Layers": image_layers},
        "Config": {
            "User": "1000:1000",
            "Volumes": null,
            "Env": ["PATH=/usr/bin", "LANG=C.UTF-8"],
            "Entrypoint": ["/usr/local/bin/tqa-runtime"],
            "Labels": {RUNTIME_REVISION_LABEL: PROTOCOL_REVISION.to_string()}
        }
    })
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|key| (*key).to_owned()).collect()
}

fn check(value: Value) -> Result<String, TemplateRefusal> {
    let inspect: ImageInspect =
        serde_json::from_value(value).map_err(|_| TemplateRefusal::NotDerived)?;
    let bases = bases().map_err(|_| TemplateRefusal::NotDerived)?;
    check_image(&inspect, &bases, &set(&["PATH", "LANG"])).map(|base| base.name().to_owned())
}

#[test]
fn sbx_u_007_derivation_records_the_most_specific_base() {
    let minimal = layers("a", 3);
    let workstation = [minimal.clone(), layers("b", 2)].concat();
    // Each base itself, an image on each base, and an image on Workstation (both bases are prefixes).
    assert_eq!(check(image(&minimal)), Ok("minimal".into()));
    assert_eq!(check(image(&workstation)), Ok("workstation".into()));
    assert_eq!(
        check(image(&[minimal.clone(), layers("c", 1)].concat())),
        Ok("minimal".into())
    );
    assert_eq!(
        check(image(&[workstation.clone(), layers("c", 2)].concat())),
        Ok("workstation".into())
    );
    // Another base, fewer layers than a base, a changed layer in the middle, and no layers at all.
    for refused in [
        layers("d", 4),
        minimal[..2].to_vec(),
        [vec![minimal[0].clone()], layers("f", 2)].concat(),
        vec![],
    ] {
        assert_eq!(check(image(&refused)), Err(TemplateRefusal::NotDerived));
    }
    let mut no_rootfs = image(&minimal);
    no_rootfs["RootFS"] = json!(null);
    assert_eq!(check(no_rootfs), Err(TemplateRefusal::NotDerived));
}

#[test]
fn sbx_u_007_the_known_bases_are_bounded_and_each_has_layers() {
    let many = (0..=MAX_BASE_IMAGES)
        .map(|index| BaseImage::new(&format!("b{index}"), layers("a", index + 1), vec![]))
        .collect();
    assert!(Bases::new(many).is_err());
    assert!(Bases::new(vec![]).is_err());
    assert!(Bases::new(vec![BaseImage::new("minimal", vec![], vec![])]).is_err());
    let most = (0..MAX_BASE_IMAGES)
        .map(|index| BaseImage::new(&format!("b{index}"), layers("a", index + 1), vec![]))
        .collect();
    assert!(Bases::new(most).is_ok());
}

/// An image derived from Minimal with one field replaced; a pointer the fixture lacks would silently change nothing.
fn with(pointer: &str, value: Value) -> Result<String, TemplateRefusal> {
    let mut document = image(&[layers("a", 3), layers("c", 1)].concat());
    assert!(
        document.pointer(pointer).is_some(),
        "the fixture lacks {pointer}"
    );
    if let Some(slot) = document.pointer_mut(pointer) {
        *slot = value;
    }
    check(document)
}

#[test]
fn sbx_u_007_the_label_volumes_and_environment_have_their_refusals() -> TestResult {
    // The protocol label: too old, absent (revision 1), malformed.
    assert_eq!(
        with(
            &format!("/Config/Labels/{RUNTIME_REVISION_LABEL}"),
            json!("1")
        ),
        Err(TemplateRefusal::RuntimeTooOld {
            needs: MIN_RUNTIME_REVISION,
            has: 1
        })
    );
    assert_eq!(
        with("/Config/Labels", json!({})),
        Err(TemplateRefusal::RuntimeTooOld {
            needs: MIN_RUNTIME_REVISION,
            has: 1
        })
    );
    assert_eq!(
        with(
            &format!("/Config/Labels/{RUNTIME_REVISION_LABEL}"),
            json!("six")
        ),
        Err(TemplateRefusal::RuntimeLabel)
    );
    // Image-declared volumes.
    assert_eq!(
        with("/Config/Volumes", json!({"/data": {}})),
        Err(TemplateRefusal::ImageVolumes)
    );
    // Environment keys the sandbox does not set: named, never valued.
    let unset = with(
        "/Config/Env",
        json!([
            "PATH=/usr/bin",
            "API_TOKEN=synthetic-image-secret",
            "MALFORMED"
        ]),
    );
    let Err(TemplateRefusal::ImageEnvUnset(named)) = &unset else {
        return Err(format!("{unset:?}").into());
    };
    assert!(
        named.contains("API_TOKEN") && named.contains("an entry without '='"),
        "{named}"
    );
    assert!(!named.contains("synthetic-image-secret"));
    Ok(())
}

#[test]
fn sbx_u_007_the_user_and_the_entrypoint_have_their_refusals() {
    // The user: numeric non-root uid:gid only (the rootfs builder's rule).
    for user in [
        "0:0",
        "1000:0",
        "0:1000",
        "root",
        "",
        "1000",
        "01000:1000",
        "1000:1000:1",
        " 1000:1000",
    ] {
        assert_eq!(
            with("/Config/User", json!(user)),
            Err(TemplateRefusal::ImageUser),
            "{user:?}"
        );
    }
    assert_eq!(
        with("/Config/User", json!("65534:65534")),
        Ok("minimal".into())
    );
    // The entrypoint must stay the base's, so the process that dials the gateway is the runtime.
    for entrypoint in [
        json!(["/bin/sh"]),
        json!(null),
        json!(["/usr/local/bin/tqa-runtime", "--x"]),
    ] {
        assert_eq!(
            with("/Config/Entrypoint", entrypoint),
            Err(TemplateRefusal::ImageEntrypoint)
        );
    }
}

#[test]
fn sbx_u_007_checks_run_in_order_and_codes_are_closed() {
    // Not derived wins over every later fact.
    let mut everything_wrong = image(&layers("d", 2));
    everything_wrong["Config"] = json!({"User": "0:0", "Volumes": {"/v": {}}, "Env": ["X=1"]});
    assert_eq!(check(everything_wrong), Err(TemplateRefusal::NotDerived));
    let codes: Vec<&str> = [
        TemplateRefusal::NotDerived,
        TemplateRefusal::RuntimeTooOld { needs: 7, has: 1 },
        TemplateRefusal::RuntimeLabel,
        TemplateRefusal::ImageVolumes,
        TemplateRefusal::ImageEnvUnset("K".into()),
        TemplateRefusal::ImageUser,
        TemplateRefusal::ImageEntrypoint,
    ]
    .iter()
    .map(TemplateRefusal::code)
    .collect();
    assert_eq!(
        codes,
        [
            "not_derived",
            "runtime_too_old",
            "runtime_label",
            "image_volumes",
            "image_env_unset",
            "image_user",
            "image_entrypoint"
        ]
    );
}

// TEST: SBX-U-017
#[test]
fn sbx_u_017_desktop_passes_only_with_its_own_build_among_the_bases() -> TestResult {
    // Desktop is built FROM Workstation and starts its own init: checked with Workstation as its most specific base,
    // its entrypoint is refused (the c16i resize repro); with its own build among the bases, it is that base.
    let minimal = layers("a", 3);
    let workstation = [minimal.clone(), layers("b", 2)].concat();
    let desktop_layers = [workstation.clone(), layers("d", 2)].concat();
    let runtime = || vec!["/usr/local/bin/tqa-runtime".to_owned()];
    let init = || vec!["/usr/local/bin/tqa-desktop-init".to_owned()];
    let mut desktop = image(&desktop_layers);
    desktop["Config"]["Entrypoint"] = json!(init());
    let inspect: ImageInspect = serde_json::from_value(desktop)?;
    let env = set(&["PATH", "LANG"]);
    let without = Bases::new(vec![
        BaseImage::new("minimal", minimal.clone(), runtime()),
        BaseImage::new("workstation", workstation.clone(), runtime()),
    ])?;
    assert_eq!(
        check_image(&inspect, &without, &env).map(|base| base.name().to_owned()),
        Err(TemplateRefusal::ImageEntrypoint)
    );
    let with = Bases::new(vec![
        BaseImage::new("minimal", minimal, runtime()),
        BaseImage::new("workstation", workstation, runtime()),
        BaseImage::new("desktop", desktop_layers, init()),
    ])?;
    assert_eq!(
        check_image(&inspect, &with, &env).map(|base| base.name().to_owned()),
        Ok("desktop".to_owned())
    );
    Ok(())
}
