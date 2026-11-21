//! A person's own changes to a workspace's memory notes (ADR-0041): write, remove and one Undo of `.memory/<name>`.
//! Listing and reading go through the files endpoints (`.memory/` is a folder of the workspace). Directory workspaces
//! only: a microVM install answers `files_unavailable` until its workspace link gains the operation. Authorization is
//! workspace access with a write share, as for uploads; the administrator role grants no content.
use super::endpoints::{blocking, workspace};
use super::path;
use crate::{GatewayError, app::State, auth::Authz};
use axum::{
    Json,
    extract::{Path as UrlPath, State as AppState},
    http::StatusCode,
};
use serde::Serialize;
use std::{os::fd::AsFd, sync::Arc};
use tqa_tools::{
    builtin::memory_note,
    workspace_tree::memory::{self, Undone},
};
use tqa_types::ConversationId;

/// What an Undo did.
#[derive(Serialize)]
pub(crate) struct UndoResponse {
    /// `restored` (the note's previous version is back) or `removed` (a new note is gone).
    outcome: &'static str,
}

/// Memory off is an ordinary folder (ADR-0041's third amendment, A11): every change here answers `409 memory_off`;
/// the files operations still work on `.memory/` as on any folder.
fn memory_on(state: &State) -> Result<(), GatewayError> {
    if state.config.memory.enabled {
        Ok(())
    } else {
        Err(GatewayError::Conflict("memory_off"))
    }
}

/// `PUT /conversations/{id}/memory/{name}`: the request body is the note's text. Its previous version is kept for one
/// Undo.
///
/// # Errors
/// `400 path_invalid` for a name that can't be a note, a text over 64 KiB or a link in the way; `400 invalid_request`
/// when the text holds what looks like a key; `403` without a write share; `409 memory_off` while memory is off;
/// `409 files_unavailable` on a microVM install; host I/O failures.
pub(crate) async fn put_note(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath((id, name)): UrlPath<(ConversationId, String)>,
    text: String,
) -> Result<StatusCode, GatewayError> {
    memory_on(&state)?;
    if memory_note::holds_key(&text) {
        return Err(GatewayError::BadRequest(
            "a memory note can't hold a key, token or password",
        ));
    }
    let root = workspace(&state, user.id, &id, true).await?;
    blocking(move || {
        let root = path::open_root(&root)?;
        let notes = memory::directory(root.as_fd())?;
        Ok(memory::write(notes.as_fd(), &name, &text)?)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /conversations/{id}/memory/{name}`: removes the note, keeping it for one Undo.
///
/// # Errors
/// `404` when there is no such note; otherwise as [`put_note`].
pub(crate) async fn delete_note(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath((id, name)): UrlPath<(ConversationId, String)>,
) -> Result<StatusCode, GatewayError> {
    memory_on(&state)?;
    let root = workspace(&state, user.id, &id, true).await?;
    blocking(move || {
        let root = path::open_root(&root)?;
        let notes = memory::directory(root.as_fd())?;
        Ok(memory::remove(notes.as_fd(), &name)?)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /conversations/{id}/memory/{name}/undo`: undoes the last change to the note, the agent's or a person's, once:
/// its kept previous version comes back, or a note that was new at that change is removed.
///
/// # Errors
/// `404` when there is nothing to undo (no kept version, and the note was not new at its last change), so an Undo
/// sent twice changes nothing the second time; otherwise as [`put_note`].
pub(crate) async fn undo_note(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath((id, name)): UrlPath<(ConversationId, String)>,
) -> Result<Json<UndoResponse>, GatewayError> {
    memory_on(&state)?;
    let root = workspace(&state, user.id, &id, true).await?;
    let undone = blocking(move || {
        let root = path::open_root(&root)?;
        let notes = memory::directory(root.as_fd())?;
        Ok(memory::undo(notes.as_fd(), &name)?)
    })
    .await?;
    Ok(Json(UndoResponse {
        outcome: match undone {
            Undone::Restored => "restored",
            Undone::Removed => "removed",
        },
    }))
}
