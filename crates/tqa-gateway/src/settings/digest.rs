//! Keyed digests for seeded settings and request fingerprints (ADR-0028): HMAC-SHA256 (RFC 2104) under a key derived
//! from the install key, so a stored digest of a low-entropy seed value cannot be checked offline without that key.
use secrecy::{ExposeSecret, SecretSlice};
use sha2::{Digest, Sha256};

const BLOCK: usize = 64;

/// HMAC-SHA256 of the concatenated `parts` under `key`.
pub(crate) fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut block = [0_u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|byte| byte ^ 0x36));
    for part in parts {
        inner.update(part);
    }
    let mut outer = Sha256::new();
    outer.update(block.map(|byte| byte ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// A digest key derived from the install key and separated by `label` from every other use of it.
pub(crate) struct KeyedDigest([u8; 32]);

impl KeyedDigest {
    pub(crate) fn new(install_key: &SecretSlice<u8>, label: &[u8]) -> Self {
        Self(hmac_sha256(install_key.expose_secret(), &[label]))
    }

    /// The digest of one item's value; the item key is bound in, so equal values under two items differ.
    pub(crate) fn of(&self, item: &str, value: &[u8]) -> [u8; 32] {
        hmac_sha256(&self.0, &[item.as_bytes(), &[0], value])
    }
}

#[cfg(test)]
mod tests {
    use super::hmac_sha256;

    /// RFC 4231 test cases 1, 2 and 6 (a key longer than the block).
    #[test]
    fn gw_i_052_hmac_matches_the_rfc_4231_vectors() {
        assert_eq!(
            crate::hex::hex(&hmac_sha256(&[0x0b; 20], &[b"Hi There"])),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            crate::hex::hex(&hmac_sha256(
                b"Jefe",
                &[b"what do ya want ", b"for nothing?"]
            )),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            crate::hex::hex(&hmac_sha256(
                &[0xaa; 131],
                &[b"Test Using Larger Than Block-Size Key - Hash Key First"]
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }
}
