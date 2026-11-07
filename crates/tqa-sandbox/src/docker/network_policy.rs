use super::NetworkMode;
use crate::SandboxError;
use bollard::models::{NetworkCreateRequest, NetworkInspect};
use std::collections::HashMap;

pub(super) fn validate_network(
    network: &NetworkInspect,
    name: &str,
    mode: NetworkMode,
) -> Result<(), SandboxError> {
    if network.driver.as_deref() != Some("bridge")
        || network.internal != Some(mode == NetworkMode::Internal)
        || network.enable_ipv6 != Some(false)
        || network
            .labels
            .as_ref()
            .and_then(|labels| labels.get("tqa.instance"))
            .map(String::as_str)
            != Some(name)
    {
        return Err(SandboxError::Preflight(
            "instance network ownership or egress mode does not match",
        ));
    }
    Ok(())
}

pub(super) fn request(name: &str, mode: NetworkMode) -> NetworkCreateRequest {
    NetworkCreateRequest {
        name: name.into(),
        driver: Some("bridge".into()),
        internal: Some(mode == NetworkMode::Internal),
        enable_ipv6: Some(false),
        labels: Some(HashMap::from([("tqa.instance".into(), name.into())])),
        ..Default::default()
    }
}
