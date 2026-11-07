use crate::GatewayError;
use serde::Deserialize;
use url::Url;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebConfig {
    pub upstream: Url,
    #[serde(default)]
    pub dev: bool,
}

impl WebConfig {
    pub fn validate(&self) -> Result<(), GatewayError> {
        let url = &self.upstream;
        if url.scheme() != "http"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(GatewayError::Configuration(
                "web upstream must be an HTTP origin without credentials",
            ));
        }
        Ok(())
    }
}
