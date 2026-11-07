//! Validated sandbox contracts and a hardened Docker Engine backend.
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as tqa_sandbox;

mod backend;
pub mod docker;
mod egress;
mod error;
mod fake;
pub mod firecracker;
mod image;
mod limits;
mod readiness;
mod retry;
mod spec;
pub mod template;

pub use backend::{
    BackendKind, EngineNetworks, Reconciliation, SandboxBackend, SandboxHandle, SandboxStats,
    reconcile,
};
pub use egress::{EgressMode, EgressPolicy, HostRule};
pub use error::{ENVIRONMENT_FAILED_EXIT, SandboxError};
pub use fake::FakeSandboxBackend;
pub use image::ImageRef;
pub use limits::{ByteSize, CpuLimit};
pub use readiness::ReadinessSource;
pub use retry::DEFAULT_CREATE_BUDGET;
pub use spec::{
    SandboxSpec, SkillsMounts, StateMount, StateSource, WorkspaceMount, WorkspaceSource,
};
