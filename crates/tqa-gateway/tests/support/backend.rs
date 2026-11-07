//! Test-owned backend captures bootstrap input in memory; it never logs credential values.
use async_trait::async_trait;
use secrecy::SecretString;
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio_util::sync::CancellationToken;
use tqa_sandbox::{
    BackendKind, FakeSandboxBackend, ReadinessSource, SandboxBackend, SandboxError, SandboxHandle,
    SandboxSpec, SandboxStats,
};
use tqa_types::ConversationId;

pub struct Backend {
    inner: FakeSandboxBackend,
    inputs: Mutex<HashMap<ConversationId, SandboxSpec>>,
    /// Each sandbox's own runtime token, by sandbox id: a task's latest spec may be a sandbox already retired.
    tokens: Mutex<HashMap<String, SecretString>>,
    pub creates: AtomicUsize,
    pub destroys: AtomicUsize,
    pub fail_create: AtomicBool,
    /// While set, `create` finds the per-sandbox network pool exhausted.
    pub pool_exhausted: AtomicBool,
    pub fail_address: AtomicBool,
    pub background_pids: AtomicUsize,
    /// When set, the runtime "exits" with this code before registering.
    pub runtime_exit: std::sync::Mutex<Option<i32>>,
    /// The next this-many starts' VM hosts die before their runtime registers (SBX-F-038).
    pub host_deaths: AtomicUsize,
    /// While set, `destroy` waits until it is cleared: the sandbox stays in retirement.
    pub hold_destroy: AtomicBool,
    pub release_destroy: tokio::sync::Notify,
    /// The next this-many destroys fail with a timeout (a loaded daemon).
    pub fail_destroys: AtomicUsize,
    /// A destroy that fails still removes the sandbox: the timeout lied (the removal completed after it).
    pub destroy_lies: AtomicBool,
    /// The engine's networks the backend reports (ADR-0035 amendment); `None` answers as an unreachable engine.
    pub engine: Mutex<Option<tqa_sandbox::EngineNetworks>>,
}

impl Backend {
    pub fn new() -> Self {
        Self::of_kind(BackendKind::Docker)
    }
    /// A recording fake that validates specs as `kind` would, for installs configured with that backend.
    pub fn of_kind(kind: BackendKind) -> Self {
        Self {
            inner: FakeSandboxBackend::new(SystemTime::UNIX_EPOCH).with_kind(kind),
            inputs: Mutex::new(HashMap::new()),
            tokens: Mutex::new(HashMap::new()),
            creates: AtomicUsize::new(0),
            destroys: AtomicUsize::new(0),
            fail_create: AtomicBool::new(false),
            pool_exhausted: AtomicBool::new(false),
            fail_address: AtomicBool::new(false),
            background_pids: AtomicUsize::new(0),
            runtime_exit: std::sync::Mutex::new(None),
            host_deaths: AtomicUsize::new(0),
            hold_destroy: AtomicBool::new(false),
            release_destroy: tokio::sync::Notify::new(),
            fail_destroys: AtomicUsize::new(0),
            destroy_lies: AtomicBool::new(false),
            engine: Mutex::new(Some(tqa_sandbox::EngineNetworks::default())),
        }
    }
    pub fn token(&self, id: &ConversationId) -> Option<SecretString> {
        self.inputs
            .lock()
            .ok()?
            .get(id)
            .map(|spec| spec.runtime_token.clone())
    }
    pub fn mounts(
        &self,
        id: &ConversationId,
    ) -> Option<(tqa_sandbox::WorkspaceMount, tqa_sandbox::StateMount)> {
        self.inputs
            .lock()
            .ok()?
            .get(id)
            .map(|spec| (spec.workspace.clone(), spec.state.clone()))
    }
    /// Whether the conversation's latest sandbox was made a Desktop's: its shared memory and its own browser tmpfs.
    pub fn desktop(&self, id: &ConversationId) -> Option<bool> {
        self.inputs.lock().ok()?.get(id).map(|spec| spec.desktop)
    }
    /// The disk size the conversation's latest sandbox was given, in bytes.
    /// The skills tiers the conversation's latest sandbox mounted.
    pub fn skills(&self, id: &ConversationId) -> Option<tqa_sandbox::SkillsMounts> {
        self.inputs
            .lock()
            .ok()?
            .get(id)
            .map(|spec| spec.skills.clone())
    }
    pub fn disk(&self, id: &ConversationId) -> Option<u64> {
        self.inputs
            .lock()
            .ok()?
            .get(id)
            .and_then(|spec| spec.disk.map(tqa_sandbox::ByteSize::bytes))
    }
    /// The conversations that currently have a live sandbox.
    /// The runtime token of the sandbox the engine holds for `id` now, as that sandbox's runtime would dial with it:
    /// unlike [`Self::token`], never a retired sandbox's (the task's latest spec).
    pub async fn live_token(
        &self,
        id: &ConversationId,
    ) -> Result<Option<SecretString>, SandboxError> {
        let live = self.inner.list().await?;
        let tokens = self.tokens.lock().map_err(|_| SandboxError::NotFound)?;
        Ok(live
            .iter()
            .rev()
            .filter(|handle| &handle.conversation_id == id)
            .find_map(|handle| tokens.get(&handle.id).cloned()))
    }

    pub async fn list_conversations(&self) -> Result<Vec<ConversationId>, SandboxError> {
        Ok(self
            .inner
            .list()
            .await?
            .into_iter()
            .map(|handle| handle.conversation_id)
            .collect())
    }
    /// The proxy URL the conversation's latest sandbox was given; `None` for an internal-only network.
    pub fn proxy(&self, id: &ConversationId) -> Option<String> {
        self.inputs
            .lock()
            .ok()?
            .get(id)?
            .egress
            .proxy_url
            .as_ref()
            .map(ToString::to_string)
    }
    /// The image pin and memory limit the conversation's latest sandbox was created with.
    pub fn created(&self, id: &ConversationId) -> Option<(String, u64)> {
        self.inputs
            .lock()
            .ok()?
            .get(id)
            .map(|spec| (spec.image.to_string(), spec.memory.bytes()))
    }
    /// The CPU (millicores), memory (bytes) and process limits the conversation's latest sandbox was created with.
    pub fn limits(&self, id: &ConversationId) -> Option<(u32, u64, u32)> {
        self.inputs
            .lock()
            .ok()?
            .get(id)
            .map(|spec| (spec.cpu.millicores(), spec.memory.bytes(), spec.pids))
    }
    /// The host directory the conversation's latest sandbox mounted as `/workspace`.
    pub fn mounted(&self, id: &ConversationId) -> Option<std::path::PathBuf> {
        let inputs = self.inputs.lock().ok()?;
        match &inputs.get(id)?.workspace.source {
            tqa_sandbox::WorkspaceSource::HostDirectory(path) => Some(path.clone()),
            _ => None,
        }
    }
    pub async fn lose(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        self.inner.destroy(handle).await
    }
    pub async fn orphan(
        &self,
        template: &ConversationId,
        conversation: ConversationId,
    ) -> Result<SandboxHandle, SandboxError> {
        let mut spec = self
            .inputs
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .get(template)
            .cloned()
            .ok_or(SandboxError::NotFound)?;
        spec.conversation_id = conversation;
        self.inner.create(spec, CancellationToken::new()).await
    }
}

#[async_trait]
impl SandboxBackend for Backend {
    fn kind(&self) -> BackendKind {
        self.inner.kind()
    }
    async fn create(
        &self,
        spec: SandboxSpec,
        cancel: CancellationToken,
    ) -> Result<SandboxHandle, SandboxError> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        if self.fail_create.load(Ordering::SeqCst) {
            return Err(SandboxError::ImageNotFound);
        }
        if self.pool_exhausted.load(Ordering::SeqCst) {
            return Err(SandboxError::Capacity);
        }
        let handle = self.inner.create(spec.clone(), cancel).await?;
        self.tokens
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .insert(handle.id.clone(), spec.runtime_token.clone());
        self.inputs
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .insert(spec.conversation_id.clone(), spec);
        Ok(handle)
    }
    async fn address(&self, handle: &SandboxHandle) -> Result<std::net::IpAddr, SandboxError> {
        if self.fail_address.load(Ordering::SeqCst) {
            return Err(SandboxError::NotFound);
        }
        self.inner.address(handle).await
    }
    async fn wait_ready(
        &self,
        handle: &SandboxHandle,
        source: &dyn ReadinessSource,
        deadline: Duration,
        cancel: CancellationToken,
    ) -> Result<(), SandboxError> {
        let exit = *self
            .runtime_exit
            .lock()
            .map_err(|_| SandboxError::NotFound)?;
        if let Some(code) = exit {
            return Err(SandboxError::RuntimeExited { code });
        }
        if self
            .host_deaths
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok()
        {
            return Err(SandboxError::HostDied);
        }
        self.inner
            .wait_ready(handle, source, deadline, cancel)
            .await
    }
    async fn stats(&self, handle: &SandboxHandle) -> Result<SandboxStats, SandboxError> {
        let mut stats = self.inner.stats(handle).await?;
        stats.pids += self.background_pids.load(Ordering::SeqCst) as u64;
        Ok(stats)
    }
    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        self.destroys.fetch_add(1, Ordering::SeqCst);
        while self.hold_destroy.load(Ordering::SeqCst) {
            self.release_destroy.notified().await;
        }
        let failing = self
            .fail_destroys
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_ok();
        if failing {
            if self.destroy_lies.load(Ordering::SeqCst) {
                self.inner.destroy(handle).await?;
            }
            return Err(SandboxError::Timeout);
        }
        self.inner.destroy(handle).await
    }
    async fn list(&self) -> Result<Vec<SandboxHandle>, SandboxError> {
        self.inner.list().await
    }
    async fn engine_networks(&self) -> Result<tqa_sandbox::EngineNetworks, SandboxError> {
        self.engine
            .lock()
            .map_err(|_| SandboxError::NotFound)?
            .clone()
            .ok_or(SandboxError::NotFound)
    }
}
