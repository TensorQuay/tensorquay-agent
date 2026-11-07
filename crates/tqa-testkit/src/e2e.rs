//! The E2E task-set harness (TEST-PLAN §16): task manifests, the approval policy, the attempt loop,
//! event-log integrity, the workspace copy and check container, failure classification and results.
//! The driver binary composes these with the API client.
pub mod attempt;
pub mod check_only;
pub mod classify;
pub mod cleanup;
pub mod docker;
pub mod driver;
pub mod environment;
pub mod integrity;
pub mod manifest;
pub mod result;
pub mod sse;
pub mod summary;
pub mod workspace;
