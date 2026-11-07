//! The Firecracker backend (ADR-0006) and its parts: configuration, the host preflight, the configuration device, the
//! per-VM launch configuration and network rules. Each VM runs in its own container on the same rootless engine as
//! Docker mode; the launcher runs as that container's namespaced root only until it executes Firecracker as the fixed
//! unprivileged uid, and the guest can never reach it.

mod acl;
mod backend;
mod config;
mod device;
mod docker_engine;
mod engine;
mod exit_record;
mod launch;
pub(crate) mod manifest;
mod preflight;
mod probe;

mod rules;
#[cfg(test)]
mod test_support;

pub use backend::FirecrackerBackend;
pub use config::{FirecrackerBackendConfig, VMM_CONTAINER_UID};
pub use engine::NetworkState;
pub use manifest::RootfsStatus;
pub use preflight::{HostProbe, Preflight, run as preflight};
