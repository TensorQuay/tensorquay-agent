//! The egress policy proxy (ARCHITECTURE §4.2, §10; ADR-0021): the only way out of a sandbox, allow-list only.
mod head;
pub(crate) mod internal;
mod keyed;
#[cfg(feature = "testing")]
pub(crate) use keyed::Closes as KeyedCloses;
mod notice;
mod proxy;
mod refusal;
mod rules;
mod scrub;
pub(crate) use rules::{
    ANY_HOST, AllowList, Decision, RuleError, RuleInput, canonical, canonical_host, guarded,
};
pub(crate) use scrub::Scrubbers;

use crate::{GatewayError, app::State};

/// The task's allow-list for `session.config` and the prompt (ADR-0035 §3): the live list at the session's start;
/// `disabled` is `[egress] disabled`, which sends an empty list.
pub(crate) fn network_snapshot(
    settings: &crate::settings::Snapshot,
    disabled: bool,
) -> tqa_types::NetworkSnapshot {
    settings.egress.snapshot(disabled)
}
use async_trait::async_trait;
use std::{net::SocketAddr, sync::Arc};

/// Name resolution for the proxy. The proxy resolves each request once and connects only to the addresses it checked.
#[async_trait]
pub trait EgressResolver: Send + Sync {
    /// Returns the addresses for `host:port`.
    ///
    /// # Errors
    /// Fails when the name cannot be resolved.
    async fn resolve(&self, host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>>;

    /// Connects to one address the proxy checked; a test resolver may redirect it to a local server.
    ///
    /// # Errors
    /// Fails when the address cannot be reached.
    async fn connect(&self, address: SocketAddr) -> std::io::Result<tokio::net::TcpStream> {
        tokio::net::TcpStream::connect(address).await
    }
}

/// The operating system resolver, used in production.
pub struct SystemResolver;

#[async_trait]
impl EgressResolver for SystemResolver {
    async fn resolve(&self, host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
        Ok(tokio::net::lookup_host((host, port)).await?.collect())
    }
}

/// Accepts sandbox connections until cancellation; each is handled by an owned, bounded task.
pub(crate) async fn serve(
    state: Arc<State>,
    listener: tokio::net::TcpListener,
    resolver: Arc<dyn EgressResolver>,
) -> Result<(), GatewayError> {
    let proxy = Arc::new(proxy::Proxy {
        state: state.clone(),
        resolver,
        notices: notice::Notices::default(),
        keyed: keyed::Limits::new(
            state.config.egress.keyed_connections,
            state.config.egress.keyed_per_sandbox,
        ),
    });
    // At most `[egress] connections` at once; more wait in the listen backlog.
    let permits = Arc::new(tokio::sync::Semaphore::new(state.config.egress.connections));
    loop {
        let permit = tokio::select! {
            () = state.cancel.cancelled() => return Ok(()),
            permit = permits.clone().acquire_owned() => permit.map_err(|_| GatewayError::Cancelled)?,
        };
        // Tunnelled streams are small writes too, and a tunnel left idle by a vanished sandbox must end (`net`).
        let (client, peer) = tokio::select! {
            () = state.cancel.cancelled() => return Ok(()),
            accepted = crate::net::accept(&listener) => accepted?,
        };
        let proxy = proxy.clone();
        let cancel = state.cancel.clone();
        state.tasks.spawn(async move {
            tokio::select! {
                () = cancel.cancelled() => (),
                () = proxy.connection(client, peer) => (),
            }
            drop(permit);
        });
    }
}
