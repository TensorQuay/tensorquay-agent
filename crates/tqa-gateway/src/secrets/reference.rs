use crate::GatewayError;

/// A store reference in the `secrets/` namespace with a bounded ASCII name.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SecretRef(String);

impl SecretRef {
    /// Returns the reference name; a reference never contains the credential value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for SecretRef {
    type Err = GatewayError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let name = value
            .strip_prefix("secrets/")
            .ok_or(GatewayError::BadRequest("invalid secret reference"))?;
        if !(1..=120).contains(&name.len())
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(GatewayError::BadRequest("invalid secret reference"));
        }
        Ok(Self(value.into()))
    }
}
