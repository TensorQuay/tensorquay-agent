//! GW-U-165: a secret reference as the log shows it.
use super::loggable;

/// A key's shape, built at run time so no key-shaped literal is in the tree.
fn shaped() -> String {
    let body: String = (0..16u8).map(|index| char::from(b'A' + index)).collect();
    format!("secrets/{}{}{body}", "AK", "IA")
}

// TEST: GW-U-165
#[test]
fn gw_u_165_a_key_shaped_name_is_never_written_out() {
    let reference = shaped();
    assert_eq!(loggable(&reference), "secrets/<a name shaped like a key>");
    assert_eq!(
        loggable(reference.trim_start_matches("secrets/")),
        "secrets/<a name shaped like a key>"
    );
    for readable in [
        "secrets/openai",
        "secrets/hf_token",
        "secrets/provider-gateway-shared-production-key",
    ] {
        assert_eq!(loggable(readable), readable);
    }
}
