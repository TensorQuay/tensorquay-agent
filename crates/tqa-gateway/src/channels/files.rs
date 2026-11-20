//! Files on a channel's posts (ADR-0014's amendment of 20 Nov 2026, A9). A voting member uploads a file to the channel
//! (the composer's limits: an attachment's size each, a workspace's uploads for the channel's files together) and posts
//! it, at most 32 to a message. People download each from its post. An agent member gets them in its workspace, flat in
//! `uploads/` as `<channel>-<seq>-<file>` and never over a file already there, when it is woken (the wake names each
//! exact path) or, failing that, when it reads the message; each is placed once, so renaming the channel moves nothing.
use super::store;
use crate::{
    GatewayError, InvalidReason,
    app::State,
    auth::{Authz, Role, User},
    files::{handed, upload as uploads},
};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State as AppState},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use std::{collections::HashMap, sync::Arc};
use tqa_types::ChannelFile;
use uuid::Uuid;

/// A message's files at most (a turn's attachments' bound).
pub(crate) const MAX_FILES: usize = tqa_types::MAX_ATTACHMENTS;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UploadQuery {
    name: String,
}

/// An upload kept for a post: its id (what a post names), its name as stored and its size.
#[derive(Serialize)]
pub(crate) struct Uploaded {
    id: Uuid,
    name: String,
    size: u64,
}

fn caller(user: &User) -> (Uuid, bool) {
    (user.id, user.role == Role::Admin)
}

/// `POST /api/v1/channels/{id}/uploads?name=`: the body is the file, kept for the caller's next post. Over the
/// per-file limit it is refused as soon as the declared length or the bytes pass it; past the channel's total, too.
///
/// # Errors
/// `404` for a channel the caller can't see, `403 not_member`/`observer`, `413 upload_too_large`, `409 uploads_full`,
/// storage failure.
pub(crate) async fn upload(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path(raw_id): Path<String>,
    Query(query): Query<UploadQuery>,
    request: axum::extract::Request,
) -> Result<(StatusCode, Json<Uploaded>), GatewayError> {
    let id = super::api::channel_id(&raw_id)?;
    let (parts, body) = request.into_parts();
    let per_file = state.config.files.upload_bytes;
    let declared = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    {
        let mut connection = state.pool.acquire().await?;
        let seen = store::visible(&mut connection, caller(&user), id).await?;
        match seen.role {
            None => return Err(GatewayError::Denied("not_member")),
            Some(role) if !role.may_post() => return Err(GatewayError::Denied("observer")),
            Some(_) => {}
        }
    }
    if declared.is_some_and(|length| length > per_file) {
        return Err(GatewayError::UploadTooLarge);
    }
    let mut bytes = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| GatewayError::BadRequest("the upload body could not be read"))?;
        if (bytes.len() + chunk.len()) as u64 > per_file {
            return Err(GatewayError::UploadTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    let size = bytes.len() as u64;
    let name = uploads::sanitise(&query.name);
    let upload_id = Uuid::now_v7();
    let total = i64::try_from(state.config.files.uploads_bytes).unwrap_or(i64::MAX);
    let mut transaction = state.pool.begin().await?;
    // One upload at a time per channel, so two can't both fit the total that only one does.
    sqlx::query!("SELECT id FROM channels WHERE id = $1 FOR UPDATE", id)
        .fetch_one(&mut *transaction)
        .await?;
    let kept_bytes = sqlx::query_scalar!(
        "SELECT COALESCE(sum(size), 0)::bigint AS \"kept!\" FROM channel_uploads WHERE channel_id = $1",
        id
    )
    .fetch_one(&mut *transaction)
    .await?;
    if kept_bytes.saturating_add(i64::try_from(size).unwrap_or(i64::MAX)) > total {
        return Err(GatewayError::Conflict("uploads_full"));
    }
    sqlx::query!(
        "INSERT INTO channel_uploads (id, channel_id, user_id, name, size) VALUES ($1, $2, $3, $4, $5)",
        upload_id,
        id,
        user.id,
        name,
        i64::try_from(size).unwrap_or(i64::MAX)
    )
    .execute(&mut *transaction)
    .await?;
    state.storage.channel_upload(id, upload_id, bytes).await?;
    transaction.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(Uploaded {
            id: upload_id,
            name,
            size,
        }),
    ))
}

/// `DELETE /api/v1/channels/{id}/uploads/{upload}`: the composer's remove: the caller's own upload that no post took
/// goes, bytes and row, and its room in the channel's total comes back.
///
/// # Errors
/// `404` for a channel the caller can't see, or an upload that isn't theirs, unposted, in this channel; storage failure.
pub(crate) async fn delete_upload(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path((raw_id, upload)): Path<(String, Uuid)>,
) -> Result<StatusCode, GatewayError> {
    let id = super::api::channel_id(&raw_id)?;
    let mut connection = state.pool.acquire().await?;
    store::visible(&mut connection, caller(&user), id).await?;
    sqlx::query_scalar!(
        "DELETE FROM channel_uploads WHERE id = $1 AND channel_id = $2 AND user_id = $3 AND posted_seq IS NULL \
         RETURNING id",
        upload,
        id,
        user.id
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(GatewayError::NotFound)?;
    state.storage.remove_channel_upload(id, upload).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// How long an upload no post took is kept.
const UNPOSTED_FOR: &str = "24 hours";

/// Removes the uploads no post took within [`UNPOSTED_FOR`], rows then bytes; answers how many went.
///
/// # Errors
/// Storage failure.
pub(crate) async fn sweep_uploads(state: &State) -> Result<usize, GatewayError> {
    let gone = sqlx::query!(
        "DELETE FROM channel_uploads WHERE posted_seq IS NULL \
         AND created_at < CURRENT_TIMESTAMP - $1::text::interval RETURNING id, channel_id",
        UNPOSTED_FOR
    )
    .fetch_all(&state.pool)
    .await?;
    for row in &gone {
        if let Err(error) = state
            .storage
            .remove_channel_upload(row.channel_id, row.id)
            .await
        {
            tracing::warn!(
                code = error.code(),
                "an unposted channel upload's bytes weren't removed"
            );
        }
    }
    Ok(gone.len())
}

/// How often unposted uploads are swept.
const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(3600);

/// Starts the hourly sweep of unposted uploads, on the gateway's task tracker, stopped by its cancel.
pub(crate) fn start(state: &Arc<State>) {
    let owned = state.clone();
    state.tasks.spawn(async move {
        let mut interval = tokio::time::interval(SWEEP_EVERY);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {biased;
                () = owned.cancel.cancelled() => break,
                _ = interval.tick() => if let Err(error) = sweep_uploads(&owned).await {
                    tracing::warn!(code = error.code(), "the channel uploads sweep failed");
                },
            }
        }
    });
}

/// The upload ids a post names, from its request body's `files` (taken out of it, so the rest is the post).
///
/// # Errors
/// `400 message_invalid` naming `files`: not a list of ids, or more than [`MAX_FILES`].
pub(crate) fn take_files(request: &mut serde_json::Value) -> Result<Vec<Uuid>, GatewayError> {
    let invalid = || GatewayError::Invalid {
        field: "files",
        reason: InvalidReason::MessageInvalid,
    };
    let Some(files) = request
        .as_object_mut()
        .and_then(|object| object.remove("files"))
    else {
        return Ok(Vec::new());
    };
    let ids = files
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|id| {
            id.as_str()
                .and_then(|id| Uuid::parse_str(id).ok())
                .ok_or_else(invalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if ids.len() > MAX_FILES {
        return Err(invalid());
    }
    Ok(ids)
}

/// Gives message `seq` of `channel` the caller's uploads `ids`, in order, in the post's transaction: each must be the
/// caller's, in this channel and not posted yet; their bytes move under the message. Answers the message's files.
///
/// # Errors
/// `400 message_invalid` naming `files` for an upload that isn't so, or named twice; storage failure.
pub(crate) async fn attach(
    state: &State,
    connection: &mut PgConnection,
    (channel, seq, caller): (Uuid, i64, Uuid),
    ids: &[Uuid],
) -> Result<Vec<ChannelFile>, GatewayError> {
    let mut files = Vec::with_capacity(ids.len());
    for (index, upload) in ids.iter().enumerate() {
        let taken = sqlx::query!(
            "UPDATE channel_uploads SET posted_seq = $4 WHERE id = $1 AND channel_id = $2 AND user_id = $3 \
             AND posted_seq IS NULL RETURNING name, size",
            upload,
            channel,
            caller,
            seq
        )
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(GatewayError::Invalid {
            field: "files",
            reason: InvalidReason::MessageInvalid,
        })?;
        let position = i16::try_from(index).map_err(|_| GatewayError::StoreUnavailable)?;
        sqlx::query!(
            "INSERT INTO channel_files (channel_id, seq, index, name, size) VALUES ($1, $2, $3, $4, $5)",
            channel,
            seq,
            position,
            taken.name,
            taken.size
        )
        .execute(&mut *connection)
        .await?;
        state
            .storage
            .keep_channel_file(channel, *upload, (seq, index))
            .await?;
        files.push(ChannelFile {
            name: taken.name,
            size: u64::try_from(taken.size).unwrap_or(0),
            path: None,
        });
    }
    Ok(files)
}

/// The files of messages `seqs` of `channel`, by seq, in order.
///
/// # Errors
/// Storage failure.
pub(crate) async fn of_messages(
    connection: &mut PgConnection,
    channel: Uuid,
    seqs: &[i64],
) -> Result<HashMap<i64, Vec<ChannelFile>>, GatewayError> {
    let mut found: HashMap<i64, Vec<ChannelFile>> = HashMap::new();
    if seqs.is_empty() {
        return Ok(found);
    }
    for row in sqlx::query!(
        "SELECT seq, name, size FROM channel_files WHERE channel_id = $1 AND seq = ANY($2) ORDER BY seq, index",
        channel,
        seqs
    )
    .fetch_all(&mut *connection)
    .await?
    {
        found.entry(row.seq).or_default().push(ChannelFile {
            name: row.name,
            size: u64::try_from(row.size).unwrap_or(0),
            path: None,
        });
    }
    Ok(found)
}

/// `GET /api/v1/channels/{id}/messages/{seq}/files/{index}`: one posted file, as a download.
///
/// # Errors
/// `404` for a channel the caller can't see or a file that isn't there; storage failure.
pub(crate) async fn download(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    Path((raw_id, seq, index)): Path<(String, i64, i16)>,
) -> Result<Response, GatewayError> {
    let id = super::api::channel_id(&raw_id)?;
    let name = {
        let mut connection = state.pool.acquire().await?;
        store::visible(&mut connection, caller(&user), id).await?;
        sqlx::query_scalar!(
            "SELECT name FROM channel_files WHERE channel_id = $1 AND seq = $2 AND index = $3",
            id,
            seq,
            index
        )
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(GatewayError::NotFound)?
    };
    let path = state
        .storage
        .channel_files(id)
        .join(seq.to_string())
        .join(index.to_string());
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| GatewayError::NotFound)?;
    let size = bytes.len() as u64;
    let mut response = Body::from(bytes).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    let name = uploads::sanitise(&name);
    if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

/// Places in `member`'s workspace the files of `channel`'s messages after `after` (up to `through`, when given) not
/// placed there yet, as `<channel>-<seq>-<file>` in `uploads/` (A9), and answers every placed path of those messages
/// by seq and index. A workspace that can't take files now (a sandbox not linked) places nothing: the member's read
/// places them later.
///
/// # Errors
/// Storage failure; a file that doesn't fit the workspace's uploads is left out.
pub(crate) async fn place(
    state: &State,
    (channel, member): (Uuid, Uuid),
    (after, through): (i64, Option<i64>),
) -> Result<Vec<(i64, i16, String)>, GatewayError> {
    let missing = sqlx::query!(
        "SELECT f.seq, f.index, f.name, ch.name AS channel_name FROM channel_files f \
         JOIN channels ch ON ch.id = f.channel_id \
         WHERE f.channel_id = $1 AND f.seq > $2 AND ($3::bigint IS NULL OR f.seq <= $3) \
           AND NOT EXISTS (SELECT 1 FROM channel_file_placements p WHERE p.channel_id = f.channel_id \
                           AND p.seq = f.seq AND p.index = f.index AND p.conversation_id = $4) \
         ORDER BY f.seq, f.index",
        channel,
        after,
        through,
        member
    )
    .fetch_all(&state.pool)
    .await?;
    if !missing.is_empty() {
        let missing: Vec<Missing> = missing
            .into_iter()
            .map(|file| Missing {
                seq: file.seq,
                index: file.index,
                name: format!("{}-{}-{}", file.channel_name, file.seq, file.name),
            })
            .collect();
        match handed::workspace(state, member).await {
            Ok(workspace) => place_each(state, &workspace, (channel, member), &missing).await?,
            Err(GatewayError::FilesUnavailable) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(sqlx::query!(
        "SELECT seq, index, path FROM channel_file_placements WHERE channel_id = $1 AND conversation_id = $2 \
         AND seq > $3 AND ($4::bigint IS NULL OR seq <= $4) ORDER BY seq, index",
        channel,
        member,
        after,
        through
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| (row.seq, row.index, row.path))
    .collect())
}

/// A file not yet in a member's workspace: its message, its place in it, and the name it gets there.
struct Missing {
    seq: i64,
    index: i16,
    name: String,
}

/// Places each missing file in `workspace` and records where; one that doesn't fit is logged and left out.
async fn place_each(
    state: &State,
    workspace: &handed::Workspace,
    (channel, member): (Uuid, Uuid),
    missing: &[Missing],
) -> Result<(), GatewayError> {
    let stored = state.storage.channel_files(channel);
    for file in missing {
        let from = stored
            .join(file.seq.to_string())
            .join(file.index.to_string());
        let path = match handed::place(state, workspace, &file.name, &from).await {
            Ok((path, _)) => path,
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "a channel file wasn't placed in a member's workspace"
                );
                continue;
            }
        };
        sqlx::query!(
            "INSERT INTO channel_file_placements (channel_id, seq, index, conversation_id, path) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
            channel,
            file.seq,
            file.index,
            member,
            path
        )
        .execute(&state.pool)
        .await?;
    }
    Ok(())
}
