use super::{SandboxManager, Slot, failure};
use crate::GatewayError;
use std::sync::Arc;
use tqa_sandbox::{
    SandboxHandle, SandboxSpec, SkillsMounts, StateMount, StateSource, WorkspaceMount,
    WorkspaceSource,
};
use tqa_types::ConversationId;

impl SandboxManager {
    /// Starts or reuses one instance and binds its bootstrap credential to its observed address.
    /// Registration and durable action delivery continue in an owned background task.
    ///
    /// # Errors
    /// Rejects deleted conversations, exhausted capacity, invalid configuration and backend failures.
    pub async fn ensure(
        self: &Arc<Self>,
        id: &ConversationId,
    ) -> Result<SandboxHandle, GatewayError> {
        self.ensure_for(id, super::Create::Work).await
    }

    /// [`Self::ensure`], saying why a new sandbox would be created.
    ///
    /// # Errors
    /// As [`Self::ensure`].
    pub(super) async fn ensure_for(
        self: &Arc<Self>,
        id: &ConversationId,
        reason: super::Create,
    ) -> Result<SandboxHandle, GatewayError> {
        self.admit_conversation(id).await?;
        let slot = self.slot(id)?;
        let permit = self
            .bounded(async {
                slot.lifecycle
                    .acquire()
                    .await
                    .map_err(|_| GatewayError::Cancelled)
            })
            .await?;
        let existing = slot
            .handle
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .clone();
        let result = if let Some(handle) = existing {
            handle
        } else {
            let handle = self.create(id, &slot, reason).await?;
            *slot
                .handle
                .lock()
                .map_err(|_| GatewayError::StoreUnavailable)? = Some(handle.clone());
            handle
        };
        drop(permit);
        self.schedule(id)?;
        Ok(result)
    }

    async fn create(
        &self,
        id: &ConversationId,
        slot: &Arc<Slot>,
        reason: super::Create,
    ) -> Result<SandboxHandle, GatewayError> {
        tracing::info!(conversation_id = %id, reason = reason.as_str(), "sandbox create begun");
        let (reservation, token) = self.services.tokens.reserve(id).await?;
        let result = async {
            let spec = self.spec(id, token).await?;
            // Refuse an image below the protocol floor before creating anything: its runtime would be refused at
            // the link handshake anyway (ADR-0026).
            if let Some(docker) = &self.backend.docker {
                docker
                    .check_runtime_image(&spec.image)
                    .await
                    .map_err(failure("create"))?;
            }
            let created = self
                .backend
                .engine
                .create(spec, self.services.cancel.clone())
                .await;
            let handle = match created {
                // No free subnet: release the slot, then the admission record, so the work waits in the queue like any
                // other. The record alone let admission count a free place while the table stayed full (PERF-020).
                Err(tqa_sandbox::SandboxError::Capacity) => {
                    self.pool_exhausted();
                    Box::pin(self.release(id, slot)).await?;
                    return Err(GatewayError::Busy);
                }
                other => other.map_err(failure("create"))?,
            };
            self.report_pool();
            if let Err(error) = self.activate(&reservation, &handle).await {
                // The backend owns private token-file cleanup along with its container.
                let cleanup = self
                    .backend
                    .engine
                    .destroy(&handle)
                    .await
                    .map_err(failure("destroy"));
                cleanup?;
                return Err(error);
            }
            // Its browser profile starts empty: nothing from an earlier sandbox to warn of at the next Take over.
            crate::screen::sandbox_created(id);
            Ok(handle)
        }
        .await;
        if result.is_err() {
            self.services.tokens.abandon(&reservation).await?;
        }
        result
    }

    async fn activate(
        &self,
        reservation: &crate::ReservationId,
        handle: &SandboxHandle,
    ) -> Result<(), GatewayError> {
        let peer = self
            .backend
            .engine
            .address(handle)
            .await
            .map_err(failure("create"))?;
        let sandbox = handle.id.parse()?;
        self.services
            .tokens
            .bind(reservation, &sandbox, peer)
            .await?;
        let id = crate::conversations::uuid(&handle.conversation_id)?;
        let result=self.bounded(async {
            let updated=sqlx::query!("UPDATE sandbox_instances SET sandbox_id=$2,phase='live',updated_at=CURRENT_TIMESTAMP WHERE conversation_id=$1 AND phase='starting'",id,handle.id).execute(&self.services.pool).await?;
            if updated.rows_affected()!=1 { return Err(GatewayError::StoreUnavailable); }
            Ok(())
        }).await;
        if result.is_err() {
            self.services.tokens.revoke(&sandbox).await?;
        }
        result
    }

    /// Every sandbox reaches the policy proxy on the gateway's own host from creation (ADR-0021: the same internal
    /// network, only proxy variables added), so the live allow-list alone decides, and a rule added later reaches it
    /// too (ADR-0028). Only `[egress] disabled = true` leaves sandboxes internal-only.
    fn egress(&self) -> Result<tqa_sandbox::EgressPolicy, GatewayError> {
        let egress = &self.config.egress;
        if egress.disabled {
            return Ok(tqa_sandbox::EgressPolicy {
                mode: tqa_sandbox::EgressMode::Internal,
                allow: Vec::new(),
                proxy_url: None,
            });
        }
        let mut proxy = url::Url::parse(&self.config.sandbox.gateway_url)
            .map_err(|_| GatewayError::Configuration("invalid runtime URL"))?;
        proxy
            .set_port(Some(egress.listen.port()))
            .map_err(|()| GatewayError::Configuration("invalid egress proxy URL"))?;
        proxy.set_path("");
        Ok(tqa_sandbox::EgressPolicy {
            mode: tqa_sandbox::EgressMode::Proxy,
            allow: Vec::new(),
            proxy_url: Some(proxy),
        })
    }

    /// The conversation's pinned template image; a legacy binding without a pin uses the runtime image.
    fn image(
        &self,
        binding: &crate::environments::Binding,
    ) -> Result<tqa_sandbox::ImageRef, GatewayError> {
        if binding.digest.is_empty() {
            return self.config.image.clone().ok_or(GatewayError::Configuration(
                "runtime.image must be digest-pinned",
            ));
        }
        binding
            .digest
            .parse()
            .map_err(|_| GatewayError::StoreUnavailable)
    }

    /// Host storage for one sandbox: directories for Docker, ext4 images the backend creates for Firecracker (the
    /// gateway never mounts or reads those, ARCHITECTURE §4.5).
    async fn storage(
        &self,
        (workspace, quota): (uuid::Uuid, u64),
        conversation: uuid::Uuid,
    ) -> Result<(WorkspaceSource, StateMount, Option<tqa_sandbox::ByteSize>), GatewayError> {
        let storage = &self.services.storage;
        if self.config.firecracker.is_some() {
            // A new image holds the workspace's own quota: the backend adds ext4's metadata (SBX-F-025). Existing
            // images keep their size.
            let quota = tqa_sandbox::ByteSize::new(quota).map_err(|_| {
                GatewayError::Configuration("a workspace quota is not a valid size")
            })?;
            return Ok((
                WorkspaceSource::BlockImage(storage.workspace_image(workspace).await?),
                StateMount {
                    source: StateSource::BlockImage(storage.state_image(conversation).await?),
                },
                Some(quota),
            ));
        }
        Ok((
            WorkspaceSource::HostDirectory(storage.workspace(workspace).await?),
            StateMount::directory(storage.runtime_state(conversation).await?),
            None,
        ))
    }

    /// The read-only skills tiers: the install tier and the conversation owner's own tier, never a sharer's. The
    /// microVM backend mounts none until it has a block image of the trees.
    async fn skills(&self, conversation: uuid::Uuid) -> Result<SkillsMounts, GatewayError> {
        if self.config.uses_block_images() {
            return Ok(SkillsMounts::default());
        }
        let pool = &self.services.pool;
        let owner = self
            .bounded(async {
                sqlx::query_scalar!(
                    "SELECT owner_id FROM conversations WHERE id=$1",
                    conversation
                )
                .fetch_optional(pool)
                .await?
                .ok_or(GatewayError::NotFound)
            })
            .await?;
        let storage = &self.services.storage;
        Ok(SkillsMounts {
            install: storage.skills_install(),
            user: Some(storage.user_skills(owner).await?),
        })
    }

    async fn spec(
        &self,
        id: &ConversationId,
        token: secrecy::SecretString,
    ) -> Result<SandboxSpec, GatewayError> {
        let uuid = crate::conversations::uuid(id)?;
        let options = &self.config.sandbox;
        let bound = self
            .bounded(crate::workspaces::of_conversation(
                &self.services.pool,
                uuid,
            ))
            .await?;
        let quota = crate::workspaces::quota(&self.config, bound.quota_bytes);
        let (workspace, state, disk) = self.storage((bound.id, quota), uuid).await?;
        let skills = self.skills(uuid).await?;
        // Boxed: its transaction would otherwise enlarge every create future (clippy::large_futures).
        Box::pin(self.bounded(crate::environments::refresh(&self.services.pool, uuid))).await?;
        let binding = self
            .bounded(crate::environments::binding(&self.services.pool, uuid))
            .await?;
        let size = sized(options, &binding);
        // The size it runs with, as its timers are recorded: an edit to the environment applies to the next one.
        self.bounded(async {
            sqlx::query!(
                "UPDATE sandbox_instances SET cpu_millicores=$2,memory_bytes=$3,pids=$4 WHERE conversation_id=$1",
                uuid,
                i32::try_from(size.cpu_millicores).map_err(|_| GatewayError::StoreUnavailable)?,
                i64::try_from(size.memory_bytes).map_err(|_| GatewayError::StoreUnavailable)?,
                i32::try_from(size.pids).map_err(|_| GatewayError::StoreUnavailable)?
            )
            .execute(&self.services.pool)
            .await?;
            Ok(())
        })
        .await?;
        Ok(SandboxSpec {
            conversation_id: id.clone(),
            workspace: WorkspaceMount {
                id: bound
                    .id
                    .to_string()
                    .parse()
                    .map_err(|_| GatewayError::StoreUnavailable)?,
                source: workspace,
            },
            state,
            image: self.image(&binding)?,
            cpu: tqa_sandbox::CpuLimit::new(size.cpu_millicores).map_err(failure("create"))?,
            memory: tqa_sandbox::ByteSize::new(size.memory_bytes).map_err(failure("create"))?,
            disk,
            pids: size.pids,
            egress: self.egress()?,
            gateway_url: url::Url::parse(&options.gateway_url)
                .map_err(|_| GatewayError::Configuration("invalid runtime URL"))?,
            runtime_token: token,
            env: options.env.clone(),
            skills,
            // The shared memory and the browser's tmpfs (ADR-0039 section 10).
            desktop: binding
                .capabilities
                .contains(&tqa_types::Capability::Desktop),
        })
    }

    pub(super) async fn retire(
        &self,
        id: &ConversationId,
        slot: &Arc<Slot>,
        reason: super::Teardown,
    ) -> Result<(), GatewayError> {
        super::teardown_begun(id, reason);
        let _permit = self
            .bounded(async {
                slot.lifecycle
                    .acquire()
                    .await
                    .map_err(|_| GatewayError::Cancelled)
            })
            .await?;
        let owned = slot
            .handle
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .clone();
        if let Some(handle) = owned.as_ref() {
            self.services.tokens.revoke(&handle.id.parse()?).await?;
            if let Err(error) = self.backend.engine.destroy(handle).await {
                // It may still exist: keep the slot and its record, and start nothing until a destroy succeeds.
                slot.unconfirmed
                    .store(true, std::sync::atomic::Ordering::Release);
                return Err(failure("destroy")(error));
            }
        }
        slot.unconfirmed
            .store(false, std::sync::atomic::Ordering::Release);
        self.release(id, slot).await
    }

    /// Frees a retired sandbox's slot: the handle, the in-memory slot, then the durable row admission counts.
    pub(super) async fn release(
        &self,
        id: &ConversationId,
        slot: &Arc<Slot>,
    ) -> Result<(), GatewayError> {
        *slot
            .handle
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)? = None;
        self.remove_slot(id, slot)?;
        crate::screen::sandbox_gone(id);
        // Keep admission closed until the old in-memory owner is gone.
        self.forget(id).await
    }
}

/// What a sandbox gets: its environment's size, or the installation's default for each field it leaves unset, never
/// above the installation's ceiling (the environment sizes design; a ceiling lowered since clamps the environment).
pub(super) struct Sized {
    pub cpu_millicores: u32,
    pub memory_bytes: u64,
    pub pids: u32,
}

pub(super) fn sized(
    options: &crate::config::SandboxConfig,
    binding: &crate::environments::Binding,
) -> Sized {
    let ceiling = &options.ceiling;
    Sized {
        cpu_millicores: binding
            .cpu_millicores
            .unwrap_or(options.cpu_millicores)
            .min(ceiling.cpu_millicores()),
        memory_bytes: binding
            .memory_bytes
            .unwrap_or(options.memory_bytes)
            .min(ceiling.memory_bytes()),
        pids: binding.pids.unwrap_or(options.pids).min(ceiling.pids()),
    }
}
