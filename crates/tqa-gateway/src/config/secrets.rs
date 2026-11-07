use crate::GatewayError;
use secrecy::{ExposeSecret, SecretString};

/// Environment-only credentials, redacted by construction and never serialized into TOML.
#[derive(Clone, Debug)]
pub struct GatewaySecrets {
    pub(crate) database_url: SecretString,
    pub(crate) bootstrap_token: Option<SecretString>,
    pub(crate) secret_files: Option<crate::SecretFiles>,
}

impl GatewaySecrets {
    /// Sets validated host file locations for the encrypted credential store.
    #[must_use]
    pub fn with_secret_files(mut self, files: crate::SecretFiles) -> Self {
        self.secret_files = Some(files);
        self
    }

    /// Validates externally supplied secret values; the database URI must use `PostgreSQL`.
    ///
    /// # Errors
    /// Returns a redacted configuration error for malformed or unbounded credentials.
    pub fn new(
        database_url: SecretString,
        bootstrap_token: Option<SecretString>,
    ) -> Result<Self, GatewayError> {
        let value = database_url.expose_secret();
        let url = url::Url::parse(value)
            .map_err(|_| GatewayError::Configuration("invalid database URL"))?;
        if value.len() > 8192
            || !matches!(url.scheme(), "postgres" | "postgresql")
            || url.host_str().is_none()
        {
            return Err(GatewayError::Configuration(
                "database URL must identify a PostgreSQL server",
            ));
        }
        if bootstrap_token
            .as_ref()
            .is_some_and(|token| !(32..=1024).contains(&token.expose_secret().len()))
        {
            return Err(GatewayError::Configuration(
                "bootstrap token must contain 32 through 1024 bytes",
            ));
        }
        Ok(Self {
            database_url,
            bootstrap_token,
            secret_files: None,
        })
    }
}
