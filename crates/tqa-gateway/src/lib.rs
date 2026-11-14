//! Authenticated gateway control plane, durable events and replayable client streams.

mod actions;
mod admin;
mod app;
mod auth;
mod channels;
mod config;
mod connectors;
mod conversations;
mod db;
mod decision_engines;
mod delivery;
mod description;
mod egress;
mod environments;
mod error;
mod events;
mod files;
mod hex;
mod host_ca;
mod host_keys;
mod io;
mod link;
pub mod logging;
mod manager;
mod mcp_proxy;
mod models;
mod net;
/// The gateway's listener rule (`TCP_NODELAY` on every accepted stream), for the binary's metrics listener.
pub use net::{Listener as NodelayListener, listener as nodelay_listener};
mod profiles;
mod purge;
mod relay;
mod request;
mod route_place;
mod routes;
mod runtime_tokens;
mod screen;
mod search;
mod secrets;
mod settings;
mod skills;
mod startup;
mod storage;
mod stream;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "agent teams, slice 1: the pure rules are used by the routes of slice 2 (ADR-0034)"
    )
)]
mod teams;
mod telemetry;
mod web;
mod wire;
mod workspaces;

pub use app::{Gateway, SandboxDependencies};
pub use auth::{Role, User};
pub use config::{GatewayConfig, GatewaySecrets, Template};
pub use connectors::{ConnectorCredential, CredentialSource};
pub use conversations::Conversation;
pub use db::MIGRATOR;
pub use egress::{EgressResolver, SystemResolver};
pub use environments::builtin::{EngineUnavailable, EnvironmentImageSource};
pub use environments::check_template_image;
pub use environments::checks::{CheckOutcome, CheckReason, CheckRequest, TemplateCheck};
pub use error::{ConnectorFailure, GatewayError, InvalidReason};
pub use events::EventStore;
pub use events::{OpenPoint, StreamOpenHold};
pub use host_ca::rotate as rotate_host_ca;
pub use link::RuntimeHeartbeat;
pub use manager::{IdleClock, SandboxManager};
pub use purge::Cycle;
pub use relay::{UNREACHABLE_GRACE, UNREACHABLE_HOLD};
pub use routes::registered_routes;
pub use runtime_tokens::{ReservationId, RuntimeTokens, SandboxId};
pub use secrets::{
    BuiltinSecretStore, FakeSecretStore, SecretEntry, SecretFiles, SecretRef, SecretStore,
    SecretStoreConfig, SecretWrite,
};
pub use settings::templates::InUse;
