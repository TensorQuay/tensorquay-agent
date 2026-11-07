//! SEC-033: a key pasted where a secret's name belongs is refused at every admin write with `name_key_shaped`, naming
//! the field and never echoing the value; a configuration file's is only warned about, by its entry, never its value.
use super::refuse_key_shaped;
use crate::{GatewayConfig, GatewayError, InvalidReason};
use axum::response::IntoResponse;

/// A made-up access key's shape, put together here so the source holds nothing key-shaped for the secrets scan (G-10).
fn pasted() -> String {
    let body: String = (0..16u8).map(|index| char::from(b'A' + index)).collect();
    format!("secrets/{}{}{body}", "AK", "IA")
}

// TEST: SEC-033
#[tokio::test]
async fn sec_033_a_key_shaped_reference_is_refused_naming_the_field_never_the_value()
-> Result<(), Box<dyn std::error::Error>> {
    let key = pasted();
    let error = refuse_key_shaped("key_ref", Some(&key))
        .err()
        .ok_or("refused")?;
    assert!(matches!(
        error,
        GatewayError::Invalid {
            field: "key_ref",
            reason: InvalidReason::NameKeyShaped
        }
    ));
    assert!(!format!("{error} {error:?}").contains(&key[8..]));
    let response = error.into_response();
    assert_eq!(response.status(), 400);
    let bytes = axum::body::to_bytes(response.into_body(), 4096).await?;
    let body: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(
        body["error"]["details"],
        serde_json::json!([{"field": "key_ref", "reason": "name_key_shaped"}])
    );
    assert!(!String::from_utf8_lossy(&bytes).contains(&key[8..]));
    for name in [
        None,
        Some("secrets/tracker-shared"),
        Some("secrets/hf_token"),
    ] {
        assert!(refuse_key_shaped("secret_ref", name).is_ok(), "{name:?}");
    }
    Ok(())
}

fn load(connector_ref: &str) -> Result<GatewayConfig, GatewayError> {
    let image = format!("sha256:{}", "a".repeat(64));
    GatewayConfig::from_toml(&format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [sandbox]\ngateway_url='http://tqa-gateway:8080'\n\
         [[connectors]]\nname='tracker'\nurl='https://mcp.tracker.example/mcp'\n\
         auth={{scheme='bearer'}}\nsecret_ref='{connector_ref}'\n"
    ))
}

// TEST: SEC-033
#[test]
fn sec_033_a_files_key_shaped_reference_starts_with_a_warning_naming_no_value()
-> Result<(), Box<dyn std::error::Error>> {
    let key = pasted();
    let warnings = load(&key)?.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("looks like a key") && !warnings[0].contains(&key[8..]));
    assert!(load("secrets/tracker-shared")?.warnings().is_empty());
    Ok(())
}

// TEST: SEC-031
// TEST: SEC-033
#[test]
fn sec_031_a_users_own_reference_is_refused_as_reserved_never_as_key_shaped() {
    use sha2::{Digest, Sha256};
    // A user's connector credential is named `connector-<64 hex>` by the gateway, from the user's id: some of those
    // digests look like a key at random. Its refusal must be the reserved one, whichever digest it is.
    let shaped: Vec<String> = (0..200u32)
        .map(|seed| {
            let digest = Sha256::digest(seed.to_le_bytes());
            format!("secrets/connector-{}", crate::hex::hex(&digest))
        })
        .filter(|reference| tqa_types::is_key_shaped_name(reference))
        .collect();
    assert!(
        !shaped.is_empty(),
        "some user's credential name looks like a key"
    );
    for reference in &shaped {
        assert!(tqa_types::is_per_user_secret(reference));
        assert!(
            super::refuse_key_shaped("key_ref", Some(reference)).is_ok(),
            "{reference} is reserved: the write's own check refuses it as such"
        );
    }
}
