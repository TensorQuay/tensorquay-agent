//! Docker Engine configuration and lifecycle operations, accessed through Bollard.

mod backend;
mod config;
mod environment_images;
mod image_files;
pub(crate) mod image_policy;
mod images;
mod io;
mod lifecycle;
mod network;
mod networks;
pub(crate) use networks::engine_networks;
mod network_policy;
mod network_sweep;
mod options;
mod pool;
mod preflight;
mod records;
mod revision;
mod runtime_state;
mod sandbox_network;
mod token;
mod workspace;

pub use backend::DockerBackend;
pub use config::container_config;
pub(crate) use config::environment;
pub use environment_images::{EnvironmentImages, ImageRecord, MAX_LISTED};
pub use image_files::{CHECK_LABEL, CHECK_PREFIX, DockerImageFiles};
pub use network_sweep::{MAX_SWEEP_NETWORKS, NetworkSweep};
pub use options::{
    DEFAULT_CREATE_TIMEOUT, DEFAULT_DESTROY_TIMEOUT, DockerBackendConfig, NetworkMode,
};
pub use pool::{
    ADDRESSES_PER_SANDBOX, Allocator, Cidr, DEFAULT_POOL, DEFAULT_SUBNET_PREFIX, MAX_SUBNET_PREFIX,
    POOL_HEADROOM, SubnetPool, routes,
};
pub use preflight::{DockerPlatform, Preflight, SeccompStatus};

#[cfg(test)]
mod network_sweep_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod pool_tests;
#[cfg(test)]
mod sandbox_network_tests;

#[cfg(test)]
mod io_tests;
