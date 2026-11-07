//! Immutable execution inputs; a call's identity never comes from model-visible flags.
use crate::{ToolError, ToolName};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tqa_types::{ActionHash, PermissionContext, canonical_action_bytes};

/// A frozen call with a digest over its tool, canonical arguments and execution context.
/// It is not cloneable; consuming it releases exactly the stored execution inputs.
#[derive(Debug)]
pub struct PreparedAction {
    tool: ToolName,
    arguments: Value,
    context: PermissionContext,
    action_hash: ActionHash,
}
impl PreparedAction {
    /// Freezes a schema-validated call. Object keys sort recursively by UTF-8 byte order;
    /// arrays retain order, and numbers/strings use `serde_json`'s compact representation.
    ///
    /// # Errors
    /// Rejects non-object arguments, zero context counters, nesting over 128 and a canonical
    /// envelope exceeding the wire frame cap. Registry schema validation remains mandatory.
    pub fn new(
        tool: ToolName,
        mut arguments: Value,
        context: PermissionContext,
    ) -> Result<Self, ToolError> {
        let bytes = canonical_action_bytes(tool.as_str(), &arguments, &context).map_err(|_| {
            ToolError::invalid("/action", "invalid prepared action context or arguments")
        })?;
        arguments.sort_all_objects();
        let action_hash = ActionHash::from_digest(Sha256::digest(bytes).into());
        Ok(Self {
            tool,
            arguments,
            context,
            action_hash,
        })
    }
    /// Borrows the exact registered name.
    #[must_use]
    pub const fn tool(&self) -> &ToolName {
        &self.tool
    }
    /// Borrows canonical arguments without permitting mutation after hashing.
    #[must_use]
    pub const fn arguments(&self) -> &Value {
        &self.arguments
    }
    /// Borrows the execution context covered by the digest.
    #[must_use]
    pub const fn context(&self) -> &PermissionContext {
        &self.context
    }
    /// Borrows the SHA-256 digest of the canonical call envelope.
    #[must_use]
    pub const fn action_hash(&self) -> &ActionHash {
        &self.action_hash
    }
    /// Consumes this preparation and returns its frozen tool name and arguments.
    #[must_use]
    pub fn into_call(self) -> (ToolName, Value) {
        (self.tool, self.arguments)
    }
}
