use crate::GatewayError;
use rand::TryRngCore;
use secrecy::SecretString;
use sha2::{Digest, Sha256};

pub(crate) fn mint() -> Result<SecretString, GatewayError> {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| GatewayError::Io)?;
    let alphabet = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in bytes {
        text.push(char::from(alphabet[usize::from(byte >> 4)]));
        text.push(char::from(alphabet[usize::from(byte & 15)]));
    }
    Ok(text.into())
}

pub(crate) fn digest(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}

pub(crate) fn credential(value: &str) -> Result<Vec<u8>, GatewayError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GatewayError::Unauthorized);
    }
    Ok(digest(value))
}

pub(crate) fn bearer(headers: &axum::http::HeaderMap) -> Result<SecretString, GatewayError> {
    let mut values = headers.get_all("authorization").iter();
    let value = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(GatewayError::Unauthorized)?;
    if values.next().is_some() {
        return Err(GatewayError::Unauthorized);
    }
    Ok(value.to_owned().into())
}

/// A 256-bit random secret in unpadded base64url (43 characters), for invitation links and personal API tokens.
///
/// # Errors
/// Fails when the operating system's random source is unavailable.
pub(crate) fn mint_url_safe() -> Result<SecretString, GatewayError> {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| GatewayError::Io)?;
    Ok(url_safe(&bytes).into())
}

/// Unpadded base64url (RFC 4648 section 5).
pub(crate) fn url_safe(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk
            .iter()
            .zip([16_u32, 8, 0])
            .fold(0_u32, |acc, (byte, shift)| {
                acc | (u32::from(*byte) << shift)
            });
        for shift in [18_u32, 12, 6, 0].into_iter().take(chunk.len() + 1) {
            text.push(char::from(ALPHABET[(value >> shift & 63) as usize]));
        }
    }
    text
}
