use std::time::{Duration, SystemTime};

use super::{
    DockerBackend, container_config, io,
    pool::Cidr,
    sandbox_network::{NETWORK_LABEL, network_name},
};
use crate::{BackendKind, SandboxError, SandboxHandle, SandboxSpec, retry::Budget};
use bollard::{
    models::{ContainerCreateBody, ContainerInspectResponse},
    query_parameters::{CreateContainerOptions, RemoveContainerOptions, StopContainerOptions},
};
use tokio_util::sync::CancellationToken;

/// How often a retry looks for an earlier attempt's create that still holds the name (SBX-U-015).
const LATE_CREATE_POLL: Duration = Duration::from_millis(250);

/// What one create of a sandbox's container found.
enum Attempted {
    /// Created and started: the container's ID.
    Started(String),
    /// An earlier attempt's create holds the name.
    NameHeld,
}

impl DockerBackend {
    pub(super) async fn create_sandbox(
        &self,
        mut spec: SandboxSpec,
        cancel: &CancellationToken,
    ) -> Result<SandboxHandle, SandboxError> {
        io::check_cancel(cancel)?;
        spec.validate(BackendKind::Docker)?;
        let _permit = io::cancellable(cancel, async {
            tokio::time::timeout(self.config.pull_timeout, self.admission.acquire())
                .await
                .map_err(|_| SandboxError::Timeout)?
                .map_err(|_| SandboxError::Backend("creation admission closed".into()))
        })
        .await?;
        self.prepare_workspace(&mut spec, cancel).await?;
        self.ensure_image(&spec, cancel).await?;
        let sampled = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| SandboxError::Preflight("host clock precedes the Unix epoch"))?;
        let created_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(sampled.as_secs());
        let name = format!(
            "{}-{}-{:x}",
            self.config.network_prefix,
            spec.conversation_id,
            sampled.as_nanos()
        );
        let token_file = self.token_path(&name)?;
        // Each sandbox gets its own network; the development mode shares the prefix network (ADR-0021 amendment).
        let own_network = self.config.network == super::NetworkMode::Internal;
        let network = if own_network {
            network_name(sampled.as_nanos())
        } else {
            self.config.network_prefix.clone()
        };
        let mut config = container_config(&spec, &network, created_at, &token_file)?;
        if own_network {
            config
                .labels
                .get_or_insert_default()
                .insert(NETWORK_LABEL.into(), network.clone());
        }
        config
            .labels
            .get_or_insert_default()
            .insert("tqa.token_id".into(), name.clone());
        config
            .labels
            .get_or_insert_default()
            .insert("tqa.instance".into(), self.config.network_prefix.clone());
        if let Some(command) = &self.config.test_command_override {
            tracing::warn!("test-only sandbox command override enabled");
            config.cmd = Some(command.clone());
        }
        io::check_cancel(cancel)?;
        self.write_token(&name, &spec.runtime_token).await?;
        let attachment = match self.attach(own_network, &network).await {
            Ok(attachment) => attachment,
            Err(error) => {
                self.remove_token(&name).await;
                return Err(error);
            }
        };
        let result = Box::pin(self.start_retried(config, &name, cancel)).await;
        match result {
            Ok(id) => {
                tracing::info!(image_digest = spec.image.digest(), "started sandbox");
                Ok(SandboxHandle {
                    kind: BackendKind::Docker,
                    id,
                    conversation_id: spec.conversation_id,
                    created_at,
                })
            }
            Err(error) => {
                self.remove_token(&name).await;
                self.undo(attachment, &network).await?;
                Err(error)
            }
        }
    }

    /// The sandbox's own network (created now), or the shared development network.
    async fn attach(&self, own_network: bool, network: &str) -> Result<Attachment, SandboxError> {
        if own_network {
            self.attach_network(network).await.map(Attachment::Own)
        } else {
            self.ensure_network().await.map(Attachment::Shared)
        }
    }

    /// Undoes what [`Self::attach`] did after a failed start.
    async fn undo(&self, attachment: Attachment, network: &str) -> Result<(), SandboxError> {
        match attachment {
            Attachment::Own(subnet) => self.discard_network(network, subnet).await,
            Attachment::Shared(true) => self.remove_empty_network().await?,
            Attachment::Shared(false) => {}
        }
        Ok(())
    }

    /// Creates and starts the sandbox's container, a busy daemon's timeout retried inside the create budget
    /// (SBX-U-014).
    pub(super) async fn start_retried(
        &self,
        config: ContainerCreateBody,
        name: &str,
        cancel: &CancellationToken,
    ) -> Result<String, SandboxError> {
        let budget = Budget::new(self.config.create_budget);
        Box::pin(self.retry.run(
            "creating a sandbox container",
            budget,
            self.config.create_timeout,
            // Each attempt's bound is `create_timeout` cut to the budget; `attempt` recomputes it after a wait.
            |number, _| {
                let config = config.clone();
                async move {
                    self.before_attempt(name, number).await?;
                    Box::pin(self.attempt(config, name, cancel, budget)).await
                }
            },
        ))
        .await
    }

    /// One attempt: the create, and while an earlier attempt's create holds the name (still in flight in the daemon,
    /// or landed late), a wait for it within the budget, its removal and the create again with a fresh bound. The
    /// conflict spends no attempt (SBX-U-014, SBX-U-015).
    async fn attempt(
        &self,
        config: ContainerCreateBody,
        name: &str,
        cancel: &CancellationToken,
        budget: Budget,
    ) -> Result<String, SandboxError> {
        loop {
            let bound = self.config.create_timeout.min(budget.left());
            match Box::pin(self.create_container_once(config.clone(), name, cancel, bound)).await? {
                Attempted::Started(id) => return Ok(id),
                Attempted::NameHeld => self.await_late_create(name, budget).await?,
            }
        }
    }

    /// Before attempt `number`: a timed-out create can land late under this same name, so a retry first removes it
    /// (this instance's only) and it is never left behind.
    async fn before_attempt(&self, name: &str, number: u32) -> Result<(), SandboxError> {
        if number > 1 {
            self.cleanup_uncertain_create(name).await?;
        }
        Ok(())
    }

    /// One attempt at the sandbox's container, each request bounded by `bound`; a held name is a timeout.
    #[cfg(test)]
    pub(super) async fn start_sandbox(
        &self,
        config: ContainerCreateBody,
        name: &str,
        cancel: &CancellationToken,
        bound: Duration,
    ) -> Result<String, SandboxError> {
        match self
            .create_container_once(config, name, cancel, bound)
            .await?
        {
            Attempted::Started(id) => Ok(id),
            Attempted::NameHeld => Err(SandboxError::Timeout),
        }
    }

    /// Creates and starts the sandbox's container, each request bounded by `bound`, or finds its name held.
    async fn create_container_once(
        &self,
        config: ContainerCreateBody,
        name: &str,
        cancel: &CancellationToken,
        bound: Duration,
    ) -> Result<Attempted, SandboxError> {
        io::check_cancel(cancel)?;
        // Mutations settle under a timeout before observing cancellation, so cleanup owns their result.
        let response = tokio::time::timeout(
            bound,
            self.docker.create_container(
                Some(CreateContainerOptions {
                    name: Some(name.into()),
                    ..Default::default()
                }),
                config,
            ),
        )
        .await;
        let created = match response {
            Ok(Ok(created)) => created,
            Ok(Err(error)) if io::conflict(&error) => return Ok(Attempted::NameHeld),
            Ok(Err(error)) => return Err(io::map_error(&error)),
            Err(_) => {
                self.cleanup_uncertain_create(name).await?;
                return Err(SandboxError::Timeout);
            }
        };
        let start = async {
            io::check_cancel(cancel)?;
            if !created.warnings.is_empty() {
                return Err(SandboxError::Preflight(
                    "Docker reported container configuration warnings",
                ));
            }
            io::request(bound, self.docker.start_container(&created.id, None)).await?;
            io::check_cancel(cancel)
        }
        .await;
        if let Err(error) = start {
            self.force_remove(&created.id).await?;
            return Err(error);
        }
        Ok(Attempted::Started(created.id))
    }

    async fn cleanup_uncertain_create(&self, name: &str) -> Result<(), SandboxError> {
        match io::request(
            self.config.operation_timeout,
            self.docker.inspect_container(name, None),
        )
        .await
        {
            Ok(inspected) => self.remove_own(name, inspected).await,
            Err(SandboxError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Waits, within `budget`, for the create in flight under `name` to land, then removes it (this instance's only).
    /// Once the budget is spent it is a timeout, as the start's own create would be.
    async fn await_late_create(&self, name: &str, budget: Budget) -> Result<(), SandboxError> {
        loop {
            let left = budget.left();
            if left.is_zero() {
                return Err(SandboxError::Timeout);
            }
            match io::request(
                self.config.operation_timeout.min(left),
                self.docker.inspect_container(name, None),
            )
            .await
            {
                Ok(inspected) => return self.remove_own(name, inspected).await,
                Err(SandboxError::NotFound) => {}
                Err(error) => return Err(error),
            }
            tokio::time::sleep(LATE_CREATE_POLL.min(budget.left())).await;
        }
    }

    /// Removes the container `name` that `inspected` describes, when this instance owns it; refuses another's.
    async fn remove_own(
        &self,
        name: &str,
        inspected: ContainerInspectResponse,
    ) -> Result<(), SandboxError> {
        let owner = inspected
            .config
            .and_then(|config| config.labels)
            .and_then(|labels| labels.get("tqa.instance").cloned());
        if owner.as_ref() != Some(&self.config.network_prefix) {
            return Err(SandboxError::Preflight(
                "container name belongs to another owner",
            ));
        }
        self.force_remove(name).await
    }

    pub(super) async fn destroy_sandbox(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        let inspected = match self.inspect_owned(handle).await {
            Ok(inspected) => inspected,
            Err(SandboxError::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        };
        let labels = inspected
            .config
            .and_then(|config| config.labels)
            .unwrap_or_default();
        let token_id = labels
            .get("tqa.token_id")
            .cloned()
            .ok_or_else(|| SandboxError::Backend("credential directory label is missing".into()))?;
        let network = labels.get(NETWORK_LABEL).cloned();
        let stopped = io::request(
            self.config.destroy_timeout,
            self.docker.stop_container(
                &handle.id,
                Some(StopContainerOptions {
                    t: Some(10),
                    ..Default::default()
                }),
            ),
        )
        .await;
        // Forced removal is still required if a busy process or an unresponsive stop timed out.
        self.force_remove(&handle.id).await?;
        self.remove_token(&token_id).await;
        if let Some(network) = network
            && let Err(error) = self.detach_network(&network, None).await
        {
            // The container is gone, so a retry would not find this network; the sweep removes it.
            tracing::warn!(
                code = error.code(),
                "sandbox network removal failed; left to the sweep"
            );
        }
        if let Err(error) = stopped
            && !matches!(error, SandboxError::NotFound | SandboxError::Timeout)
        {
            return Err(error);
        }
        tracing::info!("destroyed sandbox");
        Ok(())
    }

    async fn force_remove(&self, id: &str) -> Result<(), SandboxError> {
        match io::request(
            self.config.destroy_timeout,
            self.docker.remove_container(
                id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            ),
        )
        .await
        {
            Ok(()) | Err(SandboxError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// What a create attached the sandbox to, so a failed start undoes exactly that.
enum Attachment {
    /// The sandbox's own network, with its subnet.
    Own(Cidr),
    /// The shared development network; `true` when this create made it.
    Shared(bool),
}
