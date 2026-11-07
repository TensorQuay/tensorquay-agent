//! GW-U-120: `[runtime.builtin_images]` names the image of a built-in environment by its digest, for an install (or a
//! test stack) whose built-ins must be exactly those images, never whatever carries `tqa-runtime:<name>` on a shared
//! engine. Only the built-in names are accepted (range) and only image IDs (digest); either refusal stops the gateway.
use std::error::Error;

use crate::{GatewayConfig, GatewayError};

fn config(builtin_images: &str) -> String {
    let image = format!("sha256:{}", "a".repeat(64));
    format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n{builtin_images}"
    )
}

fn refused(toml: &str) -> Option<&'static str> {
    match GatewayConfig::from_toml(toml) {
        Err(GatewayError::Configuration(reason)) => Some(reason),
        _ => None,
    }
}

fn digest(digit: char) -> String {
    format!("sha256:{}", digit.to_string().repeat(64))
}

// TEST: GW-U-120
#[test]
fn gw_u_120_without_the_key_no_built_in_is_pinned() -> Result<(), Box<dyn Error>> {
    let parsed = GatewayConfig::from_toml(&config(""))?;
    for name in ["minimal", "workstation", "data", "desktop"] {
        assert_eq!(parsed.builtin_images().get(name), None, "{name}");
    }
    Ok(())
}

// TEST: GW-U-120
#[test]
fn gw_u_120_each_named_built_in_is_pinned_to_its_digest() -> Result<(), Box<dyn Error>> {
    let section = format!(
        "[runtime.builtin_images]\nworkstation='{}'\nminimal='{}'\n",
        digest('b'),
        digest('c')
    );
    let parsed = GatewayConfig::from_toml(&config(&section))?;
    assert_eq!(
        parsed.builtin_images().get("workstation"),
        Some(digest('b').as_str())
    );
    assert_eq!(
        parsed.builtin_images().get("minimal"),
        Some(digest('c').as_str())
    );
    assert_eq!(
        parsed.builtin_images().get("data"),
        None,
        "an unnamed built-in is not pinned"
    );
    Ok(())
}

// TEST: GW-U-120
#[test]
fn gw_u_120_only_built_in_names_are_accepted() {
    for name in ["recipe", "Workstation", "", "tqa-runtime:workstation"] {
        let section = format!("[runtime.builtin_images]\n'{name}'='{}'\n", digest('b'));
        assert_eq!(
            refused(&config(&section)),
            Some("runtime.builtin_images names only the built-ins minimal, workstation and data"),
            "{name}"
        );
    }
}

// TEST: GW-U-120
#[test]
fn gw_u_120_only_image_ids_are_accepted_never_a_tag() {
    for value in [
        "tqa-runtime:workstation".to_owned(),
        "workstation@sha256:".to_owned() + &"b".repeat(64),
        "sha256:".to_owned() + &"b".repeat(63),
        "sha256:".to_owned() + &"B".repeat(64),
        "b".repeat(64),
        String::new(),
    ] {
        let section = format!("[runtime.builtin_images]\nworkstation='{value}'\n");
        assert_eq!(
            refused(&config(&section)),
            Some(
                "runtime.builtin_images takes image IDs (sha256: and 64 lowercase hex digits), never a tag"
            ),
            "{value}"
        );
    }
}
