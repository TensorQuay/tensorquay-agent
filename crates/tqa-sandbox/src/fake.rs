use crate::{
    BackendKind, ReadinessSource, SandboxBackend, SandboxError, SandboxHandle, SandboxSpec,
    SandboxStats, readiness,
};
use async_trait::async_trait;
use futures::FutureExt;
use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Duration, SystemTime},
};
use tokio_util::sync::CancellationToken;

/// A bounded, in-memory lifecycle fake. It stores handles only, never tokens or specifications.
/// Resource samples are zero; readiness uses the caller's registration source and Tokio clock.
pub struct FakeSandboxBackend {
    created_at: SystemTime,
    kind: BackendKind,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    next: u64,
    handles: BTreeMap<String, SandboxHandle>,
    exits: BTreeMap<String, i32>,
}

impl FakeSandboxBackend {
    /// Creates an empty fake with an explicit creation clock and a 4096-sandbox bound.
    #[must_use]
    pub fn new(created_at: SystemTime) -> Self {
        Self {
            created_at,
            kind: BackendKind::Docker,
            state: Mutex::new(State::default()),
        }
    }

    /// The same fake validating and labelling as another backend, so callers can exercise that backend's specs.
    #[must_use]
    pub const fn with_kind(mut self, kind: BackendKind) -> Self {
        self.kind = kind;
        self
    }

    /// Scripts a runtime exit while retaining its owned handle for reconciliation and cleanup.
    ///
    /// # Errors
    /// Returns `NotFound` for a missing or foreign handle, or a redacted state-lock error.
    pub fn exit(&self, handle: &SandboxHandle, code: i32) -> Result<(), SandboxError> {
        self.with_state(|state| {
            if state.handles.get(&handle.id) != Some(handle) {
                return Err(SandboxError::NotFound);
            }
            state.exits.insert(handle.id.clone(), code);
            Ok(())
        })
    }

    fn running(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        self.with_state(|state| {
            if state.handles.get(&handle.id) != Some(handle) {
                return Err(SandboxError::NotFound);
            }
            match state.exits.get(&handle.id) {
                Some(&code) => Err(SandboxError::RuntimeExited { code }),
                None => Ok(()),
            }
        })
    }

    fn with_state<T>(
        &self,
        operation: impl FnOnce(&mut State) -> Result<T, SandboxError>,
    ) -> Result<T, SandboxError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| SandboxError::Backend("fake state lock poisoned".into()))?;
        operation(&mut state)
    }

    fn exists(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        self.with_state(|state| {
            if state.handles.get(&handle.id) == Some(handle) {
                Ok(())
            } else {
                Err(SandboxError::NotFound)
            }
        })
    }
}

#[async_trait]
impl SandboxBackend for FakeSandboxBackend {
    fn kind(&self) -> BackendKind {
        self.kind
    }

    async fn create(
        &self,
        spec: SandboxSpec,
        cancel: CancellationToken,
    ) -> Result<SandboxHandle, SandboxError> {
        if cancel.is_cancelled() {
            return Err(SandboxError::Cancelled);
        }
        spec.validate(self.kind)?;
        self.with_state(|state| {
            if state.handles.len() >= 4096 {
                return Err(SandboxError::Backend("fake capacity exhausted".into()));
            }
            state.next = state
                .next
                .checked_add(1)
                .ok_or_else(|| SandboxError::Backend("fake identifiers exhausted".into()))?;
            let handle = SandboxHandle {
                kind: self.kind,
                id: format!("fake-{}", state.next),
                conversation_id: spec.conversation_id,
                created_at: self.created_at,
            };
            state.handles.insert(handle.id.clone(), handle.clone());
            Ok(handle)
        })
    }

    async fn address(&self, handle: &SandboxHandle) -> Result<std::net::IpAddr, SandboxError> {
        match self.running(handle) {
            Err(SandboxError::RuntimeExited { .. }) => return Err(SandboxError::NotFound),
            result => result?,
        }
        Ok(std::net::Ipv4Addr::LOCALHOST.into())
    }

    async fn wait_ready(
        &self,
        handle: &SandboxHandle,
        source: &dyn ReadinessSource,
        deadline: Duration,
        cancel: CancellationToken,
    ) -> Result<(), SandboxError> {
        readiness::poll(
            &handle.conversation_id,
            source,
            (deadline, Duration::from_millis(100)),
            &cancel,
            &mut || async { self.running(handle) }.boxed(),
        )
        .await
    }

    async fn stats(&self, handle: &SandboxHandle) -> Result<SandboxStats, SandboxError> {
        self.exists(handle)?;
        Ok(SandboxStats::default())
    }

    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        self.with_state(|state| {
            if state.handles.get(&handle.id) == Some(handle) {
                state.handles.remove(&handle.id);
                state.exits.remove(&handle.id);
            }
            Ok(())
        })
    }

    async fn list(&self) -> Result<Vec<SandboxHandle>, SandboxError> {
        self.with_state(|state| Ok(state.handles.values().cloned().collect()))
    }
}
