//! Provider, sandbox, relay and clock fakes, fixtures, the fake model server and the
//! protocol test client for tests and benchmarks; this support crate is never shipped.

pub mod client;
pub mod deadline;
mod decision_engine;
pub mod e2e;
mod gateway;
mod jitter;
mod model_server;
pub mod perf;
mod provider;
mod provider_fakes;
mod question_server;
pub mod reach;
mod script;
mod script_wire;
pub mod stall;
pub mod standalone;
mod strict_line_breaks;
mod time;

pub use decision_engine::{EngineAnswer, EngineRequest, FakeEngineServer};
pub use gateway::{FakeGateway, FakeGatewayOptions, GatewayRequest};
pub use jitter::FixedJitter;
pub use model_server::{FakeModelOptions, FakeModelServer, RecordedRequest};
pub use provider::{FakeProvider, ProviderChunk, ProviderReply};
pub use provider_fakes::{
    FakeProfile, Refusal, Rule, Source, THOUGHT_SIGNATURE_PATH, Violation, thought_signature,
};
pub use question_server::{FakeQuestionServer, QuestionAnswer, QuestionRequest};
pub use script::{MalformedSse, Script, Scripted};
pub use strict_line_breaks::StrictLineBreaks;
pub use time::{FakeClock, FakeSleeper};

mod tool;
mod workspace;
pub use tool::{FakeTool, FakeToolReply};
pub use workspace::{WorkspaceFixture, workspace_fixture};
