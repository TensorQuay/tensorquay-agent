use super::{DockerBackend, io, network_policy::validate_network};
use crate::SandboxError;

impl DockerBackend {
    pub(super) async fn ensure_network(&self) -> Result<bool, SandboxError> {
        let name = &self.config.network_prefix;
        match io::request(
            self.config.operation_timeout,
            self.docker.inspect_network(name, None),
        )
        .await
        {
            Ok(network) => {
                validate_network(&network, name, self.config.network)?;
                Ok(false)
            }
            Err(SandboxError::NotFound) => {
                self.create_network().await?;
                tracing::info!(network_mode = ?self.config.network, "created sandbox network");
                Ok(true)
            }
            Err(error) => Err(error),
        }
    }

    async fn create_network(&self) -> Result<(), SandboxError> {
        let name = &self.config.network_prefix;
        let result = io::request(
            self.config.operation_timeout,
            self.docker
                .create_network(super::network_policy::request(name, self.config.network)),
        )
        .await;
        if let Err(error) = result {
            // A timed-out mutation can have reached the daemon; inspect ownership before cleanup.
            if error == SandboxError::Timeout {
                self.remove_empty_network().await?;
            }
            return Err(error);
        }
        Ok(())
    }

    pub(super) async fn remove_empty_network(&self) -> Result<(), SandboxError> {
        let name = &self.config.network_prefix;
        let network = match io::request(
            self.config.operation_timeout,
            self.docker.inspect_network(name, None),
        )
        .await
        {
            Ok(network) => network,
            Err(SandboxError::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        };
        if network
            .labels
            .as_ref()
            .and_then(|labels| labels.get("tqa.instance"))
            != Some(name)
        {
            return Err(SandboxError::Preflight(
                "network name belongs to another owner",
            ));
        }
        if network
            .containers
            .as_ref()
            .is_some_and(|containers| !containers.is_empty())
        {
            return Err(SandboxError::Backend(
                "network cleanup found attached containers".into(),
            ));
        }
        match io::request(
            self.config.operation_timeout,
            self.docker.remove_network(name),
        )
        .await
        {
            Ok(()) | Err(SandboxError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}
