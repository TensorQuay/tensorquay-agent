//! Bounded single-box lifecycle ownership; accepted work lives in the database, not task memory.
mod backend;
mod clock;
pub use clock::IdleClock;
pub(crate) use clock::SystemClock;
mod admission;
mod diagnostics;
mod held;
mod idle;
pub(crate) mod lease;
mod lifecycle;
mod live_quota;
mod live_usage;
mod losses;
mod networks;
mod recovery;
mod store;
mod strays;
mod teardown;
pub(crate) use store::Admission;
mod worker;
use crate::{
    GatewayConfig, GatewayError, RuntimeTokens, events::EventStore, link::LinkRegistry,
    storage::Storage,
};
pub(crate) use backend::Backend;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::AtomicBool},
};
use tokio::sync::{Notify, Semaphore};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tqa_sandbox::SandboxHandle;
use tqa_types::ConversationId;

pub(crate) struct Services {
    /// What internal egress rules may not reach; the manager renews its engine inventory (ADR-0035 amendment).
    pub internal: Arc<crate::egress::internal::Guard>,
    /// The live settings; admission and sandbox creation read the limits from one snapshot each (ADR-0028).
    pub settings: Arc<crate::settings::Settings>,
    /// Metrics for the sandbox lifecycle (bounded labels).
    pub telemetry: Arc<crate::telemetry::Telemetry>,
    pub pool: sqlx::PgPool,
    pub tokens: RuntimeTokens,
    pub links: Arc<LinkRegistry>,
    pub storage: Storage,
    pub events: EventStore,
    pub cancel: CancellationToken,
    pub tasks: TaskTracker,
    pub idle_clock: Arc<dyn IdleClock>,
}

/// Why a sandbox is torn down, as its log line says when the teardown begins (try 8 of 7 Oct 2026: the log named only a
/// finished destroy, so where a minute went couldn't be told).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Teardown {
    /// Another task of the workspace takes it over (the lease's hand-over).
    HandOver,
    /// An idle sandbox makes room for queued work.
    Eviction,
    /// The task's environment changed.
    EnvironmentSwitch,
    /// The task's workspace was deleted.
    WorkspaceDeleted,
    /// A helper its team's stop dropped before its first turn ran (GW-I-312).
    TeamStopped,
    /// Idle past its idle time.
    Idle,
    /// Past its lifetime, idle.
    Lifetime,
    /// Past its lifetime with a run going (the run resumes on a new sandbox).
    LifetimeBusy,
    /// A destroy that was never confirmed, removed before anything else.
    Unconfirmed,
    /// The link refused its runtime.
    Refused,
    /// Its workspace passed the quota (N4).
    Quota,
    /// Its start failed.
    StartFailed,
    /// Its task was deleted.
    TaskDeleted,
    /// It stopped too often to be replaced.
    RepeatedLoss,
    /// A teardown that failed, retried in the background.
    Retry,
    /// At startup, a sandbox no record holds as live, or whose runtime is gone.
    Reconcile,
    /// One that stopped unexpectedly: what the engine still holds of it goes before a replacement.
    Lost,
    /// An old container no record accounts for (the stray sweep).
    Stray,
}

impl Teardown {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::HandOver => "hand_over",
            Self::Eviction => "eviction",
            Self::EnvironmentSwitch => "environment_switch",
            Self::WorkspaceDeleted => "workspace_deleted",
            Self::TeamStopped => "team_stopped",
            Self::Idle => "idle",
            Self::Lifetime => "lifetime",
            Self::LifetimeBusy => "lifetime_busy",
            Self::Unconfirmed => "unconfirmed",
            Self::Refused => "refused",
            Self::Quota => "quota",
            Self::StartFailed => "start_failed",
            Self::TaskDeleted => "task_deleted",
            Self::RepeatedLoss => "repeated_loss",
            Self::Retry => "retry",
            Self::Reconcile => "reconcile",
            Self::Lost => "lost",
            Self::Stray => "stray",
        }
    }
}

/// Logs a teardown's beginning: every destroy, through `retire` or not, says it (the review of GW-I-259).
pub(crate) fn teardown_begun(conversation_id: &dyn std::fmt::Display, reason: Teardown) {
    tracing::info!(conversation_id = %conversation_id, reason = reason.as_str(), "sandbox teardown begun");
}

/// Why a sandbox is created, as its log line says when the create begins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Create {
    /// Work waits for it (a first start, or after a teardown).
    Work,
    /// It replaces one that stopped unexpectedly.
    Replacement,
    /// Its VM host died before its runtime registered, and the start is made once more.
    StartAgain,
}

impl Create {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::Replacement => "replacement",
            Self::StartAgain => "start_again",
        }
    }
}

/// A slot taken before its sandbox is recorded and held until the worker is scheduled, so a full table can't drop it in
/// between (GW-I-251). Dropped unused, it leaves an idle slot the table may drop.
#[must_use]
pub struct SlotHold(#[allow(dead_code)] Arc<Slot>);

struct Slot {
    handle: Mutex<Option<SandboxHandle>>,
    // Admit one lifecycle operation at a time; data guards are always released before I/O.
    lifecycle: Semaphore,
    notify: Notify,
    running: AtomicBool,
    // Set by an environment switch; the worker retires the sandbox at its next pass.
    switch: AtomicBool,
    // Why the switch was asked for (a hand-over, an eviction, an environment change), for the teardown's log line.
    switch_reason: Mutex<Teardown>,
    // Set when the link refused the sandbox's runtime (a `Refused` code; 0 is none): the worker retires it without a
    // replacement, since a new sandbox would be refused again.
    refused: std::sync::atomic::AtomicU8,
    // The live workspace quota's state for this sandbox (N4); never held across I/O.
    usage: Mutex<live_quota::LiveState>,
    // Set when the sandbox's destroy failed and it may still exist: nothing new starts for the conversation until a
    // destroy succeeds (a microVM's state image must never be used by two instances). The slot and its record are kept.
    unconfirmed: AtomicBool,
    // Set while a background teardown retry runs (`retire_later`); it schedules what follows itself.
    retrying: AtomicBool,
    // The turn of held messages in flight and the busy backoff (GW-I-129); never held across I/O.
    turns: held::Turns,
    // Set by the link when the runtime refused the turn in flight as busy; the worker puts it back.
    busy: AtomicBool,
}

impl Slot {
    fn empty() -> Self {
        Self {
            handle: Mutex::new(None),
            lifecycle: Semaphore::new(1),
            notify: Notify::new(),
            running: AtomicBool::new(false),
            switch: AtomicBool::new(false),
            switch_reason: Mutex::new(Teardown::EnvironmentSwitch),
            refused: std::sync::atomic::AtomicU8::new(0),
            usage: Mutex::new(live_quota::LiveState::default()),
            unconfirmed: AtomicBool::new(false),
            retrying: AtomicBool::new(false),
            turns: held::Turns::default(),
            busy: AtomicBool::new(false),
        }
    }

    /// Serves nothing: no sandbox, no worker, no turn in flight, and no teardown whose sandbox may still exist
    /// (`unconfirmed`, or a retry running). A switch, a refusal or a busy turn only means something beside a sandbox,
    /// so one left on a slot without one doesn't keep it.
    fn idle(&self) -> bool {
        self.handle.lock().is_ok_and(|handle| handle.is_none())
            && !self.running.load(std::sync::atomic::Ordering::Acquire)
            && !self.unconfirmed.load(std::sync::atomic::Ordering::Acquire)
            && !self.retrying.load(std::sync::atomic::Ordering::Acquire)
            && self.turns.idle()
    }
}

/// A slot the table may drop when it's full: idle, and held by the table alone, so no worker, schedule, start or
/// background teardown is using it. Readers that find no slot (a busy notice, a task accepted while retiring) answer
/// as for an idle one (GW-I-251, GW-U-171).
fn evictable(slot: &Arc<Slot>) -> bool {
    Arc::strong_count(slot) == 1 && slot.idle()
}

/// Why the link refused a sandbox's runtime; the sandbox is retired without a replacement and the owner told why.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum Refused {
    /// Below the protocol floor (ADR-0026): a new sandbox from the same image would be refused again.
    TooOld = 1,
    /// Below the revision the conversation's decision engine needs (ADR-0026's note, ADR-0013).
    EngineTooOld = 2,
    /// The conversation's decision engine is gone or disabled (ADR-0013 point 6), until an administrator restores it.
    EngineGone = 3,
    /// An agent team's lead or member below the revision that marks the gateway's wake (ADR-0026's notes of 2 and
    /// 5 Oct 2026): it would show a wake as the person's words, and a member couldn't read its brief below 12.
    TeamTooOld = 4,
    /// A conversation whose environment declares `desktop`, below the revision that brought it (ADR-0039): it would
    /// run without the `computer` tool its environment was chosen for.
    DesktopTooOld = 5,
}

impl Refused {
    /// Takes the slot's refusal, if any, clearing it.
    fn take(cell: &std::sync::atomic::AtomicU8) -> Option<Self> {
        match cell.swap(0, std::sync::atomic::Ordering::AcqRel) {
            1 => Some(Self::TooOld),
            2 => Some(Self::EngineTooOld),
            3 => Some(Self::EngineGone),
            4 => Some(Self::TeamTooOld),
            5 => Some(Self::DesktopTooOld),
            _ => None,
        }
    }

    /// The owner's notice: its code, text and whether trying again later can help.
    const fn notice(self) -> (tqa_types::ErrorCode, &'static str, bool) {
        match self {
            Self::TooOld => (
                tqa_types::ErrorCode::UnsupportedProtocol,
                "this conversation's environment runs a runtime older than this gateway supports; switch the \
                 environment, or ask an administrator to update its template",
                false,
            ),
            Self::EngineTooOld => (
                tqa_types::ErrorCode::UnsupportedProtocol,
                "this task's decision engine needs runtime revision 9; switch to an environment built on this \
                 version's runtime, or ask an administrator to update the template",
                false,
            ),
            Self::TeamTooOld => (
                tqa_types::ErrorCode::UnsupportedProtocol,
                "this team task needs runtime revision 13 to take its team's messages; switch to an environment \
                 built on this version's runtime, or ask an administrator to update the template",
                false,
            ),
            Self::DesktopTooOld => (
                tqa_types::ErrorCode::UnsupportedProtocol,
                "this task's Desktop environment needs runtime revision 14; ask an administrator to rebuild the \
                 Desktop template's image on this version, or switch the environment",
                false,
            ),
            Self::EngineGone => (
                tqa_types::ErrorCode::DecisionEngineUnavailable,
                "this task's decision engine is no longer available; an administrator must restore or re-enable it; \
                 then send a message to continue",
                true,
            ),
        }
    }
}

/// Owns one backend instance per admitted conversation and resumes durable queued actions on startup.
pub struct SandboxManager {
    config: GatewayConfig,
    backend: Backend,
    services: Services,
    slots: Mutex<HashMap<ConversationId, Arc<Slot>>>,
    admission: admission::Queue,
    networks: networks::Sweeper,
    // At most this many live workspace measurements at once, gateway-wide (N4).
    measurements: Arc<Semaphore>,
}

impl SandboxManager {
    /// The microVM backend, as the source of each template's rootfs status (ADR-0032); `None` on Docker installs.
    /// Starts the conversation's sandbox for a workspace files request (ADR-0030 D1): the ordinary admission, then the
    /// worker, which idles it out as usual. The caller has checked the requester may run the conversation.
    ///
    /// # Errors
    /// `429 busy` while it waits for a slot or its workspace; `409 conversation_deleted`; store failures.
    pub(crate) async fn start_for_files(
        self: &Arc<Self>,
        id: &ConversationId,
    ) -> Result<(), GatewayError> {
        self.admit_conversation(id).await?;
        self.schedule(id)
    }

    pub(crate) fn rootfs_source(
        &self,
    ) -> Option<Arc<tqa_sandbox::firecracker::FirecrackerBackend>> {
        self.backend.firecracker.clone()
    }

    pub(crate) fn new(config: GatewayConfig, backend: Backend, services: Services) -> Arc<Self> {
        Arc::new(Self {
            config,
            backend,
            services,
            slots: Mutex::new(HashMap::new()),
            admission: admission::Queue::default(),
            networks: networks::Sweeper::default(),
            measurements: Arc::new(Semaphore::new(live_quota::CONCURRENT_MEASUREMENTS)),
        })
    }

    fn slot(&self, id: &ConversationId) -> Result<Arc<Slot>, GatewayError> {
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        if let Some(slot) = slots.get(id) {
            return Ok(slot.clone());
        }
        let max_live =
            usize::try_from(self.config.sandbox.max_live).map_err(|_| GatewayError::Busy)?;
        if slots.len() >= max_live {
            // A slot no one else holds and that serves nothing (no sandbox, no worker, nothing in flight) is only the
            // map's: a task whose worker ended without a sandbox, waiting, cancelled or deleted. Such slots used to
            // stay until a restart and fill the table, so an admitted task got no worker and stayed "starting"
            // (GW-I-251, GW-U-171).
            slots.retain(|_, slot| !evictable(slot));
        }
        if slots.len() >= max_live {
            return Err(GatewayError::Busy);
        }
        let slot = Arc::new(Slot::empty());
        slots.insert(id.clone(), slot.clone());
        Ok(slot)
    }

    /// `id`'s slot, held so a full table can't drop it, or `None` when the table is full: what `admit` takes before it
    /// records a sandbox, as the admission pass does (`admit_waiting`).
    fn hold_slot(&self, id: uuid::Uuid) -> Result<Option<SlotHold>, GatewayError> {
        let id: ConversationId = id
            .to_string()
            .parse()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        match self.slot(&id) {
            Ok(slot) => Ok(Some(SlotHold(slot))),
            Err(GatewayError::Busy) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Tests only: holds `id`'s slot as a worker or a teardown does, so a full table can't drop it, until the hold is
    /// dropped: a slot table full of slots that serve no sandbox row (PERF-020 on c16).
    ///
    /// # Errors
    /// `429 busy` when the table is full; the slot table unavailable.
    #[cfg(feature = "testing")]
    pub fn hold_slot_for_test(&self, id: &ConversationId) -> Result<SlotHold, GatewayError> {
        self.slot(id).map(SlotHold)
    }

    /// Tests only: whether `hold` is still `id`'s slot in the table.
    #[cfg(feature = "testing")]
    pub fn table_holds_for_test(&self, id: &ConversationId, hold: &SlotHold) -> bool {
        self.slots
            .lock()
            .is_ok_and(|slots| slots.get(id).is_some_and(|slot| Arc::ptr_eq(slot, &hold.0)))
    }

    fn remove_slot(&self, id: &ConversationId, expected: &Arc<Slot>) -> Result<(), GatewayError> {
        let mut slots = self
            .slots
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        if slots
            .get(id)
            .is_some_and(|slot| Arc::ptr_eq(slot, expected))
        {
            slots.remove(id);
        }
        Ok(())
    }

    /// Asks a live sandbox to retire after its conversation switched environment; without one, nothing runs.
    ///
    /// # Errors
    /// Fails closed if the slot table is unavailable.
    /// An action accepted while the conversation's sandbox retires. Normally the retirement's end starts it; but when
    /// the old sandbox's removal was never confirmed and nothing is retrying it, the worker tries the teardown first,
    /// so the action either starts a new sandbox or fails visibly, instead of waiting for a retirement that never ends.
    pub(crate) fn accepted_while_retiring(
        self: &Arc<Self>,
        id: &ConversationId,
    ) -> Result<(), GatewayError> {
        let stranded = self
            .slots
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .get(id)
            .is_some_and(|slot| {
                slot.unconfirmed.load(std::sync::atomic::Ordering::Acquire)
                    && !slot.retrying.load(std::sync::atomic::Ordering::Acquire)
            });
        if stranded {
            self.schedule(id)?;
        }
        Ok(())
    }

    pub(crate) fn retire_for_switch(
        &self,
        id: &ConversationId,
        reason: Teardown,
    ) -> Result<(), GatewayError> {
        let slot = self
            .slots
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .get(id)
            .cloned();
        if let Some(slot) = slot {
            if let Ok(mut why) = slot.switch_reason.lock() {
                *why = reason;
            }
            slot.switch
                .store(true, std::sync::atomic::Ordering::Release);
            slot.notify.notify_one();
        }
        Ok(())
    }

    /// Asks the sandbox whose runtime the link refused to retire, without a replacement; its token is revoked, so the
    /// runtime's next dial is refused too and it stops.
    ///
    /// # Errors
    /// Fails closed if the slot table is unavailable.
    pub(crate) fn refuse(&self, id: &ConversationId, refused: Refused) -> Result<(), GatewayError> {
        let slot = self
            .slots
            .lock()
            .map_err(|_| GatewayError::StoreUnavailable)?
            .get(id)
            .cloned();
        if let Some(slot) = slot {
            slot.refused
                .store(refused as u8, std::sync::atomic::Ordering::Release);
            slot.notify.notify_one();
        }
        Ok(())
    }

    pub(crate) async fn bounded<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, GatewayError>>,
    ) -> Result<T, GatewayError> {
        crate::io::bounded(self.config.io_timeout(), &self.services.cancel, future).await
    }
}

#[cfg(test)]
#[path = "manager/failure_tests.rs"]
mod failure_tests;

#[cfg(test)]
#[path = "manager/reason_tests.rs"]
mod reason_tests;

#[cfg(test)]
#[path = "manager/slot_tests.rs"]
mod slot_tests;

/// Maps a backend error for one lifecycle stage to a bounded, secret-free sandbox failure.
fn failure(stage: &'static str) -> impl Fn(tqa_sandbox::SandboxError) -> GatewayError {
    move |error| match error {
        // The environment failed before the runtime ran (the guest init, or an entrypoint Docker can't run): the
        // agent never started, so it's the sandbox's failure, not the runtime's (GW-U-086).
        tqa_sandbox::SandboxError::RuntimeExited {
            code: tqa_sandbox::ENVIRONMENT_FAILED_EXIT,
        } => {
            tracing::warn!(
                stage,
                code = tqa_sandbox::ENVIRONMENT_FAILED_EXIT,
                "the sandbox's environment failed before the runtime started"
            );
            GatewayError::Sandbox {
                stage,
                cause: "environment failed before the runtime started",
            }
        }
        tqa_sandbox::SandboxError::RuntimeExited { code } => GatewayError::RuntimeExited { code },
        // A second death in a row (the first is started once more): a start failure, as an exit without a record was.
        tqa_sandbox::SandboxError::HostDied => GatewayError::RuntimeExited { code: -1 },
        other => {
            if let tqa_sandbox::SandboxError::InvalidSpec { field, reason } = &other {
                // The reason is bounded and names settings, never values; it tells the operator what to change.
                tracing::warn!(stage, field, %reason, "sandbox specification refused");
            }
            if let tqa_sandbox::SandboxError::Preflight(reason) = &other {
                // A fixed text naming the missing capability or setting; without it the operator sees only the cause.
                tracing::error!(stage, reason, "sandbox backend preflight failed");
            }
            GatewayError::Sandbox {
                stage,
                cause: cause(&other),
            }
        }
    }
}

const fn cause(error: &tqa_sandbox::SandboxError) -> &'static str {
    use tqa_sandbox::SandboxError;
    match error {
        SandboxError::Preflight(_) => "backend preflight failed",
        SandboxError::ImageNotFound => "image not found",
        SandboxError::ImageDigestMismatch => "image digest mismatch",
        SandboxError::InvalidSpec { .. } => "invalid sandbox specification",
        SandboxError::Backend(_) => "backend operation failed",
        SandboxError::NotFound => "instance not found",
        SandboxError::RuntimeExited { .. } => "runtime exited",
        SandboxError::HostDied => "VM host ended before registration",
        SandboxError::RuntimeTooOld { .. } => "runtime image too old",
        SandboxError::Timeout => "deadline exceeded",
        SandboxError::Cancelled => "cancelled",
        SandboxError::Capacity => "network pool exhausted",
    }
}
