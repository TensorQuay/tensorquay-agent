use crate::GatewayError;
use axum::http::HeaderMap;
use url::Url;

const COMMON_PASSWORDS: &[&str] = &[
    "password1234",
    "passwordpassword",
    "123456789012",
    "qwertyuiop12",
    "letmeinletmein",
    "correcthorsebatterystaple",
];

pub(crate) fn normalize_email(email: &str) -> Result<String, GatewayError> {
    let email = email.trim();
    let mut pieces = email.split('@');
    let local = pieces.next().unwrap_or_default();
    let domain = pieces.next().unwrap_or_default();
    if !(3..=254).contains(&email.len())
        || !email.is_ascii()
        || local.is_empty()
        || domain.is_empty()
        || pieces.next().is_some()
        || email
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        return Err(GatewayError::BadRequest(
            "email must be a bounded ASCII address",
        ));
    }
    Ok(email.to_ascii_lowercase())
}

pub(crate) fn validate_password(password: &str, email: &str) -> Result<(), GatewayError> {
    let lower = password.to_lowercase();
    if password.chars().count() < 12
        || password.len() > 1024
        || lower.contains(email)
        || COMMON_PASSWORDS.contains(&lower.as_str())
    {
        return Err(GatewayError::BadRequest(
            "password must contain at least 12 characters, omit the email and avoid common passwords",
        ));
    }
    Ok(())
}

pub(crate) fn check_origin(
    headers: &HeaderMap,
    origin: &Url,
    required: bool,
) -> Result<(), GatewayError> {
    let mut values = headers.get_all("origin").iter();
    let accepted = match values.next() {
        Some(value) if values.next().is_none() => value
            .to_str()
            .ok()
            .is_some_and(|value| value == origin.origin().ascii_serialization()),
        Some(_) => false,
        None => {
            !required
                || headers
                    .get("sec-fetch-site")
                    .is_some_and(|v| v == "same-origin")
        }
    };
    if !accepted {
        return Err(GatewayError::Forbidden(
            "request origin does not match public_origin",
        ));
    }
    Ok(())
}
