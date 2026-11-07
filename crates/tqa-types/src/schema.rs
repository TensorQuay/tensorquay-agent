//! Deterministic schema definitions for every protocol-v1 wire type.

use schemars::generate::SchemaSettings;
use serde_json::{Value, json};

use crate::{
    ClientFrame, ConversationId, ErrorCode, Event, RunId, Seq, ServerFrame, ToolCallId, WorkspaceId,
};

/// Returns a draft-7 schema document whose definitions are the shared wire contracts.
/// Consumers select a definition with a JSON reference; the document itself is a registry.
#[must_use]
pub fn protocol_schema() -> Value {
    let mut generator = SchemaSettings::draft07().into_generator();
    generator.subschema_for::<ClientFrame>();
    generator.subschema_for::<ServerFrame>();
    generator.subschema_for::<Event>();
    generator.subschema_for::<ErrorCode>();
    generator.subschema_for::<ConversationId>();
    generator.subschema_for::<RunId>();
    generator.subschema_for::<ToolCallId>();
    generator.subschema_for::<WorkspaceId>();
    generator.subschema_for::<Seq>();
    generator.subschema_for::<crate::GitPushOperationRequest>();
    generator.subschema_for::<crate::GitPushAuthorization>();
    generator.subschema_for::<crate::GitPushReceipt>();
    generator.subschema_for::<crate::GitPushRelayError>();
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "TensorQuay protocol v1",
        "definitions": generator.take_definitions(true),
    })
}

/// Returns the draft-7 schema of the `decision/v1` request and response (ADR-0013 point 5), exported beside the
/// protocol's as `schema/decision-v1.json`.
#[must_use]
pub fn decision_schema() -> Value {
    let mut generator = SchemaSettings::draft07().into_generator();
    generator.subschema_for::<crate::decision_v1::Request>();
    generator.subschema_for::<crate::decision_v1::Response>();
    generator.subschema_for::<crate::DecisionExposure>();
    json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "title": "TensorQuay decision/v1",
        "definitions": generator.take_definitions(true),
    })
}
