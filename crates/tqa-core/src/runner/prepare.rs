//! Every call the model makes runs (ADR-0035): the loop only freezes its identity, which the Git relay binds a push
//! to, and answers a call it can't freeze with an error result.
use super::{
    journal,
    session::{Execution, Stop},
};
use crate::CoreError;
use tqa_tools::{PreparedAction, ToolSpec};
use tqa_types::{PermissionContext, ToolStatus};

impl Execution<'_> {
    pub(super) async fn prepare(
        &mut self,
        call: &journal::Call,
        arguments: serde_json::Value,
        spec: &ToolSpec,
    ) -> Result<Option<PreparedAction>, Stop> {
        let context = PermissionContext {
            conversation_id: self.runner.conversation_id.clone(),
            run_id: self.run_id()?,
            step: self
                .context
                .step
                .ok_or(CoreError::Internal("missing call step"))?,
            attempt: self.current_attempt()?.number,
            call_id: call.id.clone(),
        };
        match PreparedAction::new(spec.name.clone(), arguments, context) {
            Ok(action) => Ok(Some(action)),
            Err(error) => {
                self.result(
                    call,
                    ToolStatus::Error,
                    error.model_message(),
                    super::tools::Beside::default(),
                )
                .await?;
                Ok(None)
            }
        }
    }
}
