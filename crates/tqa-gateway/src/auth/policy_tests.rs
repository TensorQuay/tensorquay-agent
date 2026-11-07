use super::policy::{check_origin, normalize_email, validate_password};
use axum::http::{HeaderMap, HeaderValue};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn gw_s_021_origin_policy_is_exact_and_does_not_trust_a_conflicting_hint() -> TestResult {
    let origin = url::Url::parse("https://gateway.example")?;
    let mut headers = HeaderMap::new();
    assert!(check_origin(&headers, &origin, true).is_err());
    assert!(check_origin(&headers, &origin, false).is_ok());
    headers.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
    assert!(check_origin(&headers, &origin, true).is_ok());
    headers.insert(
        "origin",
        HeaderValue::from_static("https://foreign.example"),
    );
    assert!(check_origin(&headers, &origin, true).is_err());
    headers.insert(
        "origin",
        HeaderValue::from_static("https://gateway.example"),
    );
    assert!(check_origin(&headers, &origin, true).is_ok());
    headers.append(
        "origin",
        HeaderValue::from_static("https://gateway.example"),
    );
    assert!(check_origin(&headers, &origin, true).is_err());
    Ok(())
}

#[test]
fn gw_i_001_email_and_password_policies_are_bounded() -> TestResult {
    assert!(super::Role::parse("unknown").is_err());
    assert_eq!(
        normalize_email(" ADMIN@EXAMPLE.INVALID ")?,
        "admin@example.invalid"
    );
    for email in [
        "",
        "broken",
        "@example.invalid",
        "x@",
        "x@y@z",
        "x y@example.invalid",
    ] {
        assert!(normalize_email(email).is_err(), "{email}");
    }
    for password in [
        "short",
        "password1234",
        "123456789012",
        "ADMIN@EXAMPLE.INVALID",
    ] {
        assert!(validate_password(password, "admin@example.invalid").is_err());
    }
    assert!(validate_password(&"x".repeat(1025), "admin@example.invalid").is_err());
    assert!(validate_password("distinct synthetic passphrase", "admin@example.invalid").is_ok());
    Ok(())
}
