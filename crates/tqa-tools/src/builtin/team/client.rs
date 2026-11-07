//! The runtime's agent-team routes as a client, so the tools can be tested against a fake.
use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// A refused or failed call: the gateway's stable code and a content-free sentence, shown as `<code>: <message>`.
pub use super::super::channel::ChannelFailure as TeamFailure;

/// A read's answer: the gateway's JSON (`messages`, `last_seq`, `more`, `hint`?), read tolerantly by the tools.
pub type ReadAnswer = Value;

/// `/runtime/teams/...`. Each call names its idempotency key and its run (`X-TQA-Run-ID`, which every route a tool
/// calls takes). Every call ends promptly when `cancel` fires.
#[async_trait]
pub trait TeamClient: Send + Sync {
    /// `POST /runtime/teams/members` under the call's `(key, run)`.
    async fn start(
        &self,
        body: &Value,
        call: (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure>;
    /// `POST /runtime/teams/messages` under the call's `(key, run)`.
    async fn post(
        &self,
        body: &Value,
        call: (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure>;
    /// `POST /runtime/teams/files` under the call's `(key, run)` (`take_files`).
    async fn take_files(
        &self,
        body: &Value,
        call: (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure>;
    /// `POST /runtime/teams/done` under the call's `(key, run)`.
    async fn done(
        &self,
        body: &Value,
        call: (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure>;
    /// `POST /runtime/teams/members/{name}/stop` under the call's `(key, run)` (`stop_member`).
    async fn stop_member(
        &self,
        name: &str,
        body: &Value,
        call: (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure>;
    /// `GET /runtime/teams/members/{name}/transcript?since&limit` for the run `run` (`read_helper`).
    async fn transcript(
        &self,
        name: &str,
        (since, limit): (Option<u64>, Option<u32>),
        run: &str,
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure>;
    /// `GET /runtime/teams/acceptance` for the run `run`: the administrator's acceptance check, which the lead's
    /// `done` runs first (ADR-0034 point 10); `None` when none is set. A client without the route asks nothing.
    async fn acceptance(
        &self,
        _run: &str,
        _cancel: &CancellationToken,
    ) -> Result<Option<String>, TeamFailure> {
        Ok(None)
    }
    /// `GET /runtime/teams/messages?limit&wait` for the run `run`, under the call's `key`.
    async fn read(
        &self,
        (limit, wait): (Option<u32>, Option<u32>),
        (key, run): (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<ReadAnswer, TeamFailure>;
}
