//! A member's report files and the lead's `take_files` (ADR-0034 point 6, slice 5). A report's files are read from the
//! member's workspace before the report's transaction, into `handed/<member>/` (the storage root), and kept under the
//! report's seq once it is stored; a refused or replayed report keeps nothing. The lead places them in its own
//! `uploads/` as a person's attachments, named `<member>-<path>`: nothing in a workspace is ever overwritten, and no
//! new workspace operation or runtime revision is needed. The stored copies go with the member task's purge, and a
//! deleted member's files can't be taken from the moment of the delete.
use std::{path::PathBuf, sync::Arc};

use axum::http::{HeaderMap, StatusCode};
use serde_json::Value;
use sqlx::PgConnection;
use tqa_types::{FilesTaken, HandedFile, PlacedFile, TakeFiles, TeamMessageBody, TeamPost};
use uuid::Uuid;

use super::{
    roles::{self, Role, Route},
    store,
};
use crate::{
    GatewayError, app::State, conversations::idempotent::Keyed, files::handed, storage::Storage,
};

/// A report's files read from the member's workspace, waiting for the report's seq. Dropped without [`Self::keep`],
/// the copies are removed.
pub(crate) struct Collected {
    files: Vec<HandedFile>,
    member: Uuid,
    staging: std::sync::Mutex<Option<PathBuf>>,
    storage: Storage,
    tasks: tokio_util::task::TaskTracker,
}

impl Collected {
    fn none(state: &State, member: Uuid) -> Self {
        Self {
            files: Vec::new(),
            member,
            staging: std::sync::Mutex::new(None),
            storage: state.storage.clone(),
            tasks: state.tasks.clone(),
        }
    }

    /// The files as the report lists them.
    pub(crate) fn files(&self) -> &[HandedFile] {
        &self.files
    }

    /// Keeps the copies under the report's `seq`, in the transaction that stores it.
    ///
    /// # Errors
    /// A storage failure.
    pub(crate) async fn keep(&self, seq: u64) -> Result<(), GatewayError> {
        let staging = self.staging.lock().map_err(|_| GatewayError::Io)?.take();
        let Some(staging) = staging else {
            return Ok(());
        };
        let kept = self.storage.keep_handed(&staging, self.member, seq).await;
        if kept.is_err() {
            *self.staging.lock().map_err(|_| GatewayError::Io)? = Some(staging);
        }
        kept
    }
}

impl Drop for Collected {
    fn drop(&mut self) {
        let staging = self.staging.get_mut().ok().and_then(Option::take);
        if let Some(staging) = staging {
            self.tasks.spawn(remove(self.storage.clone(), staging));
        }
    }
}

/// Removes a report's unused file copies, in the background.
async fn remove(storage: Storage, staging: PathBuf) {
    if let Err(error) = storage.remove_handed(&staging).await {
        tracing::warn!(
            code = error.code(),
            "a report's unused file copies weren't removed"
        );
    }
}

/// Reads a member's report files from its workspace: each a regular file within `files.upload_bytes`, together within
/// `files.uploads_bytes`, in the report's order. Anything but a member's report with files reads nothing (the post
/// itself refuses what it must).
///
/// # Errors
/// The report's `files` rule (`invalid_request`); per file, `path_invalid`, `not_found`, `upload_too_large`,
/// `uploads_full`, `file_changed` or `files_unavailable` naming it; storage failures.
pub(crate) async fn collect(
    state: &Arc<State>,
    member: Uuid,
    post: &TeamPost,
) -> Result<Collected, GatewayError> {
    let mut collected = Collected::none(state, member);
    let TeamPost::Report { files, .. } = post else {
        return Ok(collected);
    };
    if files.is_empty() || !is_member(state, member).await? {
        return Ok(collected);
    }
    super::validate::report_files(files)?;
    let workspace = handed::workspace(state, member).await?;
    let staging = state.storage.handed_staging(member).await?;
    *collected.staging.get_mut().map_err(|_| GatewayError::Io)? = Some(staging.clone());
    let limits = handed::limits(state);
    let mut total: u64 = 0;
    for (index, path) in files.iter().enumerate() {
        let (size, sha256) = handed::copy_out(
            state,
            &workspace,
            path,
            &staging.join(index.to_string()),
            limits.per_file,
        )
        .await
        .map_err(|error| error.for_file(path))?;
        total = total.saturating_add(size);
        if total > limits.per_workspace {
            return Err(GatewayError::Conflict("uploads_full").for_file(path));
        }
        collected.files.push(HandedFile {
            name: path.clone(),
            size,
            sha256,
        });
    }
    Ok(collected)
}

/// The hand-back's copies (GW-I-311): `wanted` (path and size, from the member's workspace directory `root`) read in
/// order while each is within `files.upload_bytes` and all within `files.uploads_bytes`. Those over a limit, and those
/// that can't be read, are answered apart, each for what it is (16p live W1: an unreadable file was said to be over
/// the limits), never refused.
///
/// # Errors
/// A storage failure.
pub(crate) async fn collect_changed(
    state: &State,
    member: Uuid,
    root: &std::path::Path,
    wanted: &[(String, u64)],
) -> Result<(Collected, Left), GatewayError> {
    let mut collected = Collected::none(state, member);
    let mut left = Left::default();
    if wanted.is_empty() {
        return Ok((collected, left));
    }
    let staging = state.storage.handed_staging(member).await?;
    *collected.staging.get_mut().map_err(|_| GatewayError::Io)? = Some(staging.clone());
    let limits = handed::limits(state);
    let mut total: u64 = 0;
    for (path, size) in wanted {
        if *size > limits.per_file || total.saturating_add(*size) > limits.per_workspace {
            left.over.push(path.clone());
            continue;
        }
        let index = collected.files.len();
        match handed::copy_out_directory(
            root,
            path,
            &staging.join(index.to_string()),
            limits.per_file,
        )
        .await
        {
            Ok((size, sha256)) => {
                total = total.saturating_add(size);
                collected.files.push(HandedFile {
                    name: path.clone(),
                    size,
                    sha256,
                });
            }
            Err(GatewayError::UploadTooLarge) => left.over.push(path.clone()),
            Err(_) => left.unread.push(path.clone()),
        }
    }
    Ok((collected, left))
}

/// The files a hand-back didn't attach, by why.
#[derive(Default)]
pub(crate) struct Left {
    /// Over `files.upload_bytes`, or past `files.uploads_bytes` together.
    pub(crate) over: Vec<String>,
    /// Couldn't be read (gone, changed while read, or refused by the workspace's path rules).
    pub(crate) unread: Vec<String>,
}

/// Whether `conversation` is a member of a team that hasn't ended.
async fn is_member(state: &State, conversation: Uuid) -> Result<bool, GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        let caller = store::caller(&mut connection, conversation).await?;
        Ok(caller.is_some_and(|caller| caller.role == Role::Member))
    })
    .await
}

/// A stored report, as `take_files` needs it.
struct Report {
    member: Uuid,
    name: String,
    files: Vec<HandedFile>,
    /// The member task is still there (not deleted).
    present: bool,
}

/// What the first transaction of `take_files` found.
enum Found {
    /// The answer stored for this key.
    Answered(Value),
    /// The report to place files from.
    Report(Report),
}

/// `take_files` for the lead `lead`: places the report's files (all, or those named) in the lead's `uploads/`. The
/// report is read under the key in one transaction, the files placed outside any, and the answer recorded in a second;
/// a retry that finds the answer recorded gets it. Files placed before a failure stay, and a retry places them again.
///
/// # Errors
/// `403 not_a_member`/`not_lead`; `409 team_stopped` or `not_current_run` (`run` isn't the lead's current run); `422 unknown_reference` (`report_seq`); `400 invalid_request`
/// (the `files` rule); per file `404 not_found` (not in the report, or the member deleted), `413 upload_too_large`,
/// `409 uploads_full` or `files_unavailable` naming it; `idempotency_key_reused`; storage failures.
pub(crate) async fn take(
    state: &Arc<State>,
    (lead, run): (Uuid, &str),
    headers: &HeaderMap,
    request: &TakeFiles,
) -> Result<Value, GatewayError> {
    if let Some(files) = &request.files {
        super::validate::report_files(files)?;
    }
    let route = format!("POST /runtime/teams/files as {lead}");
    let found = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let keyed = keyed(&mut transaction, lead, headers, &route, request).await?;
        if let Some((_, body)) = keyed.begin(state, &mut transaction).await? {
            return Ok(Found::Answered(body));
        }
        let report = report(&mut transaction, (lead, run), request.report_seq).await?;
        transaction.commit().await?;
        Ok(Found::Report(report))
    })
    .await?;
    let report = match found {
        Found::Answered(body) => return Ok(body),
        Found::Report(report) => report,
    };
    let wanted =
        super::files_rules::wanted(&report.files, report.present, request.files.as_deref())?;
    let workspace = handed::workspace(state, lead).await?;
    let stored = state
        .storage
        .handed(report.member)
        .join(request.report_seq.to_string());
    let mut placed = Vec::with_capacity(wanted.len());
    for index in wanted {
        let file = &report.files[index];
        let from = stored.join(index.to_string());
        if !tokio::fs::try_exists(&from).await.unwrap_or(false) {
            return Err(GatewayError::NotFound.for_file(&file.name));
        }
        let name = super::files_rules::placed_name(&report.name, &file.name);
        let (path, size) = handed::place(state, &workspace, &name, &from)
            .await
            .map_err(|error| error.for_file(&file.name))?;
        placed.push(PlacedFile {
            name: file.name.clone(),
            path,
            size,
        });
    }
    let body = serde_json::to_value(FilesTaken { placed })?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let keyed = keyed(&mut transaction, lead, headers, &route, request).await?;
        if let Some((_, answered)) = keyed.begin(state, &mut transaction).await? {
            return Ok(answered);
        }
        keyed
            .record(&mut transaction, StatusCode::OK, &body)
            .await?;
        transaction.commit().await?;
        Ok(body)
    })
    .await
}

/// The call's key, scoped to the lead's conversation as the team's other writes are.
async fn keyed<'a>(
    connection: &mut PgConnection,
    lead: Uuid,
    headers: &'a HeaderMap,
    route: &str,
    request: &TakeFiles,
) -> Result<Keyed<'a>, GatewayError> {
    let (owner, _) = crate::channels::store::conversation_author(connection, lead)
        .await?
        .ok_or(GatewayError::Unauthorized)?;
    Keyed::for_conversation(owner, lead, headers, route.to_owned(), request)
}

/// The report `seq` of the lead's team (running, stopping or ended), with its member and whether that member's task
/// is still there; the call must come from the lead's current `run`.
async fn report(
    connection: &mut PgConnection,
    (lead, run): (Uuid, &str),
    seq: u64,
) -> Result<Report, GatewayError> {
    // A team that has ended still lets its lead take its reports' files (GW-I-311: the gateway hands a stopped
    // helper's files back at the end, and the lead's next turn takes them).
    let caller = match store::caller(connection, lead).await? {
        Some(caller) => Some(caller),
        None => store::ended(connection, lead).await?,
    };
    let Some(caller) = caller else {
        roles::allowed(None, Route::TakeFiles)?;
        return Err(GatewayError::Denied("not_a_member"));
    };
    roles::allowed(Some(caller.role), Route::TakeFiles)?;
    if store::current_run(connection, lead).await?.as_deref() != Some(run) {
        return Err(GatewayError::Conflict("not_current_run"));
    }
    let unknown = GatewayError::UnknownReference {
        field: "report_seq",
    };
    let seq = i64::try_from(seq).map_err(|_| unknown.clone())?;
    let row = sqlx::query!(
        "SELECT m.author_id, m.author_name, m.body, EXISTS (SELECT 1 FROM agent_team_members a \
         JOIN conversations c ON c.id = a.conversation_id WHERE a.team_id = $3 \
         AND a.conversation_id::text = m.author_id AND c.state <> 'deleted') AS \"present!\" \
         FROM channel_messages m WHERE m.channel_id = $1 AND m.seq = $2 AND m.type = 'report'",
        caller.channel,
        seq,
        caller.team
    )
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(unknown)?;
    let TeamMessageBody::Report { files, .. } =
        serde_json::from_value(row.body).map_err(|_| GatewayError::StoreUnavailable)?
    else {
        return Err(GatewayError::StoreUnavailable);
    };
    Ok(Report {
        member: Uuid::parse_str(&row.author_id).map_err(|_| GatewayError::StoreUnavailable)?,
        // A name goes only with the member task's purge, when `present` is false and nothing is placed.
        name: row.author_name.unwrap_or_default(),
        files,
        present: row.present,
    })
}
