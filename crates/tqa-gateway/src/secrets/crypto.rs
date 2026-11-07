use super::{SecretRef, SecretStoreConfig};
use crate::GatewayError;
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::TryRngCore;
use secrecy::{ExposeSecret, SecretSlice, SecretString};

pub(super) struct Cipher(XChaCha20Poly1305);
impl Cipher {
    pub fn new(key: &SecretSlice<u8>) -> Result<Self, GatewayError> {
        XChaCha20Poly1305::new_from_slice(key.expose_secret())
            .map(Self)
            .map_err(|_| GatewayError::SecretUnavailable)
    }
    pub fn encrypt(
        &self,
        reference: &SecretRef,
        value: &SecretString,
        config: &SecretStoreConfig,
    ) -> Result<(Vec<u8>, Vec<u8>), GatewayError> {
        config.value(value.expose_secret())?;
        let mut nonce = [0_u8; 24];
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| GatewayError::SecretUnavailable)?;
        let payload = Payload {
            msg: value.expose_secret().as_bytes(),
            aad: reference.as_str().as_bytes(),
        };
        let encrypted = self
            .0
            .encrypt(&XNonce::from(nonce), payload)
            .map_err(|_| GatewayError::SecretUnavailable)?;
        Ok((nonce.to_vec(), encrypted))
    }
    pub fn decrypt(
        &self,
        reference: &SecretRef,
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<SecretString, GatewayError> {
        let nonce: [u8; 24] = nonce
            .try_into()
            .map_err(|_| GatewayError::SecretUnavailable)?;
        let payload = Payload {
            msg: ciphertext,
            aad: reference.as_str().as_bytes(),
        };
        let plaintext: SecretSlice<u8> = self
            .0
            .decrypt(&XNonce::from(nonce), payload)
            .map_err(|_| GatewayError::SecretUnavailable)?
            .into();
        let text = std::str::from_utf8(plaintext.expose_secret())
            .map_err(|_| GatewayError::SecretUnavailable)?;
        Ok(text.to_owned().into())
    }
}
