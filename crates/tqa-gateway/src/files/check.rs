//! The message endpoint's attachment check: a message names only uploaded files of its task's workspace.
use super::endpoints::Tree;
use super::path::{self, WorkspacePath};
use crate::{GatewayError, app::State, auth::User};
use std::os::fd::AsFd;
use tqa_types::ConversationId;

/// Each attachment must name an existing regular file under the task's workspace `uploads/`, reached without a
/// link; checked before the message is stored. On microVM installs a live sandbox answers (ADR-0030); none is started
/// here, since the caller holds the task's row: [`prepare_attachments`] starts one first.
///
/// # Errors
/// `400 attachment_missing`, the workspace access errors of the file routes, `409 sandbox_not_ready` on a microVM
/// install without a live sandbox, host I/O failures.
pub(crate) async fn check_attachments(
    state: &State,
    user: &User,
    id: &ConversationId,
    attachments: &[String],
) -> Result<(), GatewayError> {
    if attachments.is_empty() {
        return Ok(());
    }
    let paths = attachments
        .iter()
        .map(|reference| {
            reference
                .strip_prefix("uploads/")
                .filter(|name| !name.contains('/'))
                .ok_or(GatewayError::AttachmentMissing)
                .and_then(|_| {
                    WorkspacePath::parse(reference).map_err(|_| GatewayError::AttachmentMissing)
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let root = match super::endpoints::tree(state, user, id, (false, false)).await? {
        Tree::Directory(root) => root,
        Tree::Runtime(link) => {
            return if super::linked::all_files(state, &link, &paths).await? {
                Ok(())
            } else {
                Err(GatewayError::AttachmentMissing)
            };
        }
    };
    super::endpoints::blocking(move || {
        let root = match path::open_root(&root) {
            Ok(root) => root,
            Err(GatewayError::NotFound) => return Err(GatewayError::AttachmentMissing),
            Err(error) => return Err(error),
        };
        for path in &paths {
            match path::open_file(root.as_fd(), path) {
                Ok(_) => {}
                Err(GatewayError::NotFound | GatewayError::PathInvalid) => {
                    return Err(GatewayError::AttachmentMissing);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    })
    .await
}

/// On a microVM install, makes sure a runtime can answer a message's attachment check before the action's
/// transaction takes the task's row: the owner's sandbox is started here when none is live (ADR-0030). Best effort:
/// a failure leaves the check itself to answer, after the replay lookup, so a replayed key keeps its first answer.
pub(crate) async fn prepare_attachments(
    state: &State,
    user: &User,
    id: &ConversationId,
    attachments: &[String],
) {
    if attachments.is_empty() || !state.config.uses_block_images() {
        return;
    }
    if let Err(error) = super::endpoints::tree(state, user, id, (true, false)).await {
        tracing::debug!(
            code = error.code(),
            "no runtime for the attachment check yet"
        );
    }
}
