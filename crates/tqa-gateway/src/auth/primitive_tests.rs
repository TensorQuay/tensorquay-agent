use super::{LoginRate, Passwords, token};
use crate::{GatewayConfig, GatewayError};
use secrecy::ExposeSecret;
use std::time::Duration;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn gw_i_040_bearer_authentication_rejects_missing_malformed_and_duplicate_headers() -> TestResult {
    use axum::http::{HeaderMap, HeaderValue};
    let mut headers = HeaderMap::new();
    assert_eq!(
        token::bearer(&headers).err(),
        Some(GatewayError::Unauthorized)
    );
    for value in [
        HeaderValue::from_static("Basic invalid"),
        HeaderValue::from_bytes(&[0xff])?,
    ] {
        headers.insert("authorization", value);
        assert_eq!(
            token::bearer(&headers).err(),
            Some(GatewayError::Unauthorized)
        );
    }
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer synthetic"),
    );
    assert_eq!(token::bearer(&headers)?.expose_secret(), "synthetic");
    headers.append("authorization", HeaderValue::from_static("Bearer another"));
    assert_eq!(
        token::bearer(&headers).err(),
        Some(GatewayError::Unauthorized)
    );
    Ok(())
}

#[test]
fn gw_i_001_tokens_are_random_bounded_and_stored_as_hashes() -> TestResult {
    let first = token::mint()?;
    let second = token::mint()?;
    assert_ne!(first.expose_secret(), second.expose_secret());
    assert_eq!(first.expose_secret().len(), 64);
    assert_eq!(token::credential(first.expose_secret())?.len(), 32);
    assert_eq!(
        token::credential(first.expose_secret())?,
        token::digest(first.expose_secret())
    );
    for input in ["", "short", &"z".repeat(64), &"a".repeat(65)] {
        assert_eq!(token::credential(input), Err(GatewayError::Unauthorized));
    }
    assert!(!format!("{first:?}").contains(first.expose_secret()));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn gw_i_001_rate_window_has_exact_time_and_address_bounds() -> TestResult {
    let limit = LoginRate::new(1);
    let first = std::net::Ipv4Addr::LOCALHOST.into();
    let other = std::net::Ipv6Addr::LOCALHOST.into();
    for _ in 0..10 {
        limit.check(first, tokio::time::Instant::now())?;
    }
    // Past its 10 attempts an address is told the window's real wait, as a rate limit (`Too many attempts…`), not a
    // busy server's fixed second (WEB-E2E-025 on 16d read "Something went wrong… Try again in 1 s").
    assert_eq!(
        limit.check(first, tokio::time::Instant::now()),
        Err(GatewayError::RateLimited(60))
    );
    // A full table is the server's state, not this address's: still busy.
    assert_eq!(
        limit.check(other, tokio::time::Instant::now()),
        Err(GatewayError::Busy)
    );
    tokio::time::advance(Duration::from_millis(59_500)).await;
    assert_eq!(
        limit.check(first, tokio::time::Instant::now()),
        Err(GatewayError::RateLimited(1)),
        "the wait is rounded up, never 0"
    );
    tokio::time::advance(Duration::from_secs(1)).await;
    limit.check(other, tokio::time::Instant::now())?;
    Ok(())
}

#[tokio::test]
async fn gw_i_001_password_hashes_pin_argon2id_and_verify_without_plaintext_storage() -> TestResult
{
    let config = GatewayConfig::from_toml(
        "listen='127.0.0.1:0'\npublic_origin='https://gateway.example'\nmodels_path='models.toml'\n[storage]\nroot='/gateway-storage'",
    )?;
    let tasks = TaskTracker::new();
    let cancel = CancellationToken::new();
    let passwords = Passwords::new(&config, tasks.clone(), cancel.clone());
    let hash = passwords.hash("synthetic passphrase".into()).await?;
    assert!(hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
    assert!(!hash.contains("synthetic passphrase"));
    assert!(
        passwords
            .verify("synthetic passphrase".into(), hash.clone())
            .await?
    );
    assert!(
        !passwords
            .verify("different passphrase".into(), hash.clone())
            .await?
    );
    assert!(
        !passwords
            .verify(
                "synthetic passphrase".into(),
                hash.replace("m=19456", "m=1073741824")
            )
            .await?
    );
    assert!(
        !passwords
            .verify("synthetic passphrase".into(), "invalid".into())
            .await?
    );
    cancel.cancel();
    assert_eq!(
        passwords.hash("synthetic passphrase".into()).await,
        Err(GatewayError::Cancelled)
    );
    tasks.close();
    tokio::time::timeout(Duration::from_secs(10), tasks.wait()).await?;
    Ok(())
}

#[test]
fn gw_i_046_url_safe_secrets_are_unpadded_base64url_of_256_bits()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes: Vec<u8> = (0..32).collect();
    assert_eq!(
        token::url_safe(&bytes),
        "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
    );
    assert_eq!(token::url_safe(&[0xfb, 0xff]), "-_8");
    let first = token::mint_url_safe()?;
    let second = token::mint_url_safe()?;
    assert_eq!(first.expose_secret().len(), 43);
    assert_ne!(first.expose_secret(), second.expose_secret());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn gw_i_046_the_31st_token_failure_in_a_minute_is_refused_until_the_window_ends()
-> Result<(), Box<dyn std::error::Error>> {
    let failures = super::rate::TokenFailures::new(2);
    let address = std::net::IpAddr::from([127, 0, 0, 1]);
    let start = tokio::time::Instant::now();
    for _ in 0..30 {
        failures.check(address, start)?;
        failures.fail(address, start)?;
    }
    assert!(failures.check(address, start).is_err(), "the 31st attempt");
    assert!(
        failures
            .check(address, start + std::time::Duration::from_secs(59))
            .is_err()
    );
    failures.check(address, start + std::time::Duration::from_mins(1))?;
    // A full table refuses new addresses rather than growing without bound.
    // (The check above evicted the expired window, so the table is empty again.)
    failures.fail(std::net::IpAddr::from([127, 0, 0, 2]), start)?;
    failures.fail(std::net::IpAddr::from([127, 0, 0, 3]), start)?;
    assert!(
        failures
            .fail(std::net::IpAddr::from([127, 0, 0, 4]), start)
            .is_err()
    );
    Ok(())
}
