use crate::{GatewayError, GatewaySecrets};
use secrecy::SecretString;

impl GatewaySecrets {
    /// Reads database/bootstrap credentials and the install-key/optional seed file locations.
    ///
    /// # Errors
    /// Returns a redacted configuration error for missing, non-Unicode or invalid values.
    pub fn from_env() -> Result<Self, GatewayError> {
        let database = std::env::var("TQA_DATABASE_URL")
            .map_err(|_| GatewayError::Configuration("TQA_DATABASE_URL is required"))?;
        let bootstrap = match std::env::var("TQA_BOOTSTRAP_TOKEN") {
            Ok(value) => Some(SecretString::from(value)),
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(GatewayError::Configuration(
                    "TQA_BOOTSTRAP_TOKEN must be Unicode",
                ));
            }
        };
        let install_key = std::env::var_os("TQA_INSTALL_KEY_FILE").ok_or(
            GatewayError::Configuration("TQA_INSTALL_KEY_FILE is required"),
        )?;
        let seed = std::env::var_os("TQA_SECRETS_SEED_FILE").map(std::path::PathBuf::from);
        let files = crate::SecretFiles::new(install_key.into(), seed)?;
        Ok(Self::new(SecretString::from(database), bootstrap)?.with_secret_files(files))
    }
}
