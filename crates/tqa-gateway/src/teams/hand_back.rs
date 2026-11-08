//! A stopped helper's work, handed back by the gateway (GW-I-311; a live check before 17.0, 18 Oct 2026: two helpers
//! stopped at their budget share mid-run, never having seen the 80 % ask, and their files stayed in their own copies).
//! A helper that stops before it reported since its last assignment, at its share of the budget or by its team's end,
//! gets a report in the team's log written by the gateway: its changed files from its own copy are attached, within the
//! attachment limits, and the rest named, so the lead takes them with `take_files` as any report's, after the end too,
//! and the end note lists them. The model isn't needed for any of it. A helper on the lead's own files has nothing to
//! attach: its writes are already there.
use std::path::Path;

use sqlx::PgConnection;
use tqa_types::{EndReason, TeamMessageBody, TeamMessageType};
use uuid::Uuid;

use crate::{GatewayError, app::State, files::handed};

/// Why a helper stops before it reported.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Why {
    /// The team reached the helpers' share of its budget (`helpers_stop_percent`).
    Share,
    /// The team ended.
    End(EndReason),
}

impl Why {
    const fn words(self) -> &'static str {
        match self {
            Self::Share => "the team reached its helpers' share of the budget",
            Self::End(EndReason::Done) => "the lead finished the team",
            Self::End(EndReason::Stopped) => "the team was stopped",
            Self::End(EndReason::Budget) => "the team's budget ran out",
            Self::End(EndReason::Deadline) => "the team's time ran out",
            Self::End(EndReason::Idle) => "the team stopped after a while with nothing happening",
            Self::End(EndReason::LeadDeleted) => "the lead's task was deleted",
            Self::End(EndReason::PersonDisabled) => "the team's person was disabled",
        }
    }
}

/// The most files a hand-back looks at in one copy, and the most it names that it couldn't attach.
const SCANNED: usize = 1000;
const NAMED: usize = 20;

/// Stops `team`'s working helpers at the helpers' share of its budget, through the one stop path (their runs
/// cancelled, their queued team turns dropped), as `stopped`, never `failed` (a live check before 17.0: failed, they
/// were woken again every 15 s), then hands back what each changed.
///
/// # Errors
/// A store failure, or a step past its bound.
pub(crate) async fn at_share(state: &State, team: Uuid) -> Result<(), GatewayError> {
    let helpers = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT conversation_id FROM agent_team_members WHERE team_id = $1 \
             AND state NOT IN ('done', 'stopped', 'failed') ORDER BY name",
            team
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    if helpers.is_empty() {
        return Ok(());
    }
    for helper in &helpers {
        // One cancel that fails leaves the others to stop; its run then ends on the relay's refusal.
        if let Err(error) = super::stop::cancel_member(state, *helper).await {
            tracing::warn!(member = %helper, code = error.code(), "a helper's run wasn't cancelled at its share");
        }
    }
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        sqlx::query!(
            "UPDATE agent_team_members SET state = 'stopped' WHERE conversation_id = ANY($1) \
             AND state NOT IN ('done', 'stopped')",
            &helpers
        )
        .execute(&state.pool)
        .await?;
        Ok(())
    })
    .await?;
    if let Some((channel, seq)) = hand_back(state, team, &helpers, Why::Share).await? {
        state.channels.published(channel, seq);
    }
    Ok(())
}

/// One helper as the hand-back reads it.
struct Helper {
    conversation: Uuid,
    name: String,
    own_copy: bool,
    channel: Uuid,
    /// Where its work since its last assignment begins: the latest of its first run's start (the copy leaves no older
    /// time), its last assignment and its own last report. A file of its copy changed after it is that work; what it
    /// reported before, which the lead may have taken and changed, is never handed back again (the peer review).
    since: Option<time::OffsetDateTime>,
}

/// Hands back, for each of `members` that hasn't reported (nor been handed back) since its last assignment, what it
/// changed, as the gateway's report to the lead. Answers the last report's channel and seq, to publish.
///
/// # Errors
/// A store failure, or a step past its bound. A helper whose files can't be read is handed back without them, saying
/// so.
pub(crate) async fn hand_back(
    state: &State,
    team: Uuid,
    members: &[Uuid],
    why: Why,
) -> Result<Option<(Uuid, u64)>, GatewayError> {
    let helpers = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        unreported(&mut connection, team, members).await
    })
    .await?;
    let mut last = None;
    for helper in helpers {
        last = Some((helper.channel, one(state, &helper, why).await?));
    }
    Ok(last)
}

/// The helpers among `members` with no report since their last assignment, the gateway's included.
async fn unreported(
    connection: &mut PgConnection,
    team: Uuid,
    members: &[Uuid],
) -> Result<Vec<Helper>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT m.conversation_id, m.name, m.workspace, t.channel_id, \
         GREATEST((SELECT MIN(e.created_at) FROM events e WHERE e.conversation_id = m.conversation_id \
          AND e.lifecycle IN ('run.started', 'run.resumed')), \
          (SELECT MAX(a.created_at) FROM channel_messages a WHERE a.channel_id = t.channel_id AND a.type = 'assign' \
           AND a.to_conversation = m.conversation_id), \
          (SELECT MAX(r.created_at) FROM channel_messages r WHERE r.channel_id = t.channel_id AND r.type = 'report' \
           AND r.author_kind = 'conversation' AND r.author_id = m.conversation_id::text)) AS since \
         , EXISTS (SELECT 1 FROM events e WHERE e.conversation_id = m.conversation_id \
          AND e.lifecycle IN ('run.started', 'run.resumed')) AS \"started!\" \
         FROM agent_team_members m JOIN agent_teams t ON t.id = m.team_id \
         WHERE m.team_id = $1 AND m.conversation_id = ANY($2) AND NOT EXISTS (SELECT 1 FROM channel_messages r \
         WHERE r.channel_id = t.channel_id AND r.type = 'report' AND r.author_id = m.conversation_id::text \
         AND r.seq > COALESCE((SELECT MAX(a.seq) FROM channel_messages a WHERE a.channel_id = t.channel_id \
         AND a.type = 'assign' AND a.to_conversation = m.conversation_id), 0)) ORDER BY m.name",
        team,
        members
    )
    .fetch_all(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Helper {
            conversation: row.conversation_id,
            name: row.name,
            own_copy: row.workspace == "own_copy",
            channel: row.channel_id,
            since: row.since.filter(|_| row.started),
        })
        .collect())
}

/// Hands back one helper: its files read and kept under the report's seq, in the transaction that stores it.
async fn one(state: &State, helper: &Helper, why: Why) -> Result<u64, GatewayError> {
    let name = &helper.name;
    let mut text = format!(
        "The gateway hands back {name}'s work: {name} stopped before it reported ({}).",
        why.words()
    );
    let mut wanted = Vec::new();
    let mut root = std::path::PathBuf::new();
    let mut note = String::new();
    if !helper.own_copy {
        text.push_str(
            " It worked in your own files, so its changes are already there: nothing to attach.",
        );
    } else if let Some(since) = helper.since {
        match handed::workspace(state, helper.conversation).await {
            Ok(handed::Workspace::Directory(directory)) => {
                let (found, capped) = changed(directory.clone(), since).await?;
                wanted = found;
                root = directory;
                if capped {
                    let _ = std::fmt::Write::write_fmt(
                        &mut note,
                        format_args!(" It looked at the first {SCANNED} files of its copy only."),
                    );
                }
            }
            // A block-image workspace is read through its runtime, gone with the helper's run.
            _ => text.push_str(" Its files couldn't be read here: they are in its own task."),
        }
    } else {
        text.push_str(" It hadn't started, so it changed nothing.");
    }
    let readable = helper.own_copy && helper.since.is_some() && root != Path::new("");
    let (collected, left) =
        super::files::collect_changed(state, helper.conversation, &root, &wanted).await?;
    if readable {
        attached(&mut text, collected.files().len(), &left, state);
        text.push_str(&note);
        text.push_str(" Files it deleted aren't listed.");
    }
    let body = TeamMessageBody::Report {
        text: super::people_rules::cut(&text, TeamMessageType::Report.max_text_bytes()),
        files: collected.files().to_vec(),
    };
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let seq = super::store::handed_back(
            &mut transaction,
            helper.channel,
            (helper.conversation, name),
            &body,
        )
        .await?;
        collected.keep(seq).await?;
        transaction.commit().await?;
        Ok(seq)
    })
    .await
}

/// The report's sentences on what is attached and what isn't, naming the limits.
fn attached(text: &mut String, count: usize, left: &super::files::Left, state: &State) {
    use std::fmt::Write as _;
    match count {
        0 if left.over.is_empty() && left.unread.is_empty() => {
            text.push_str(" It changed no files in its own copy.");
        }
        0 => {}
        1 => {
            text.push_str(" Attached: 1 file it changed in its own copy; take it with take_files.");
        }
        n => {
            let _ = write!(
                text,
                " Attached: {n} files it changed in its own copy; take them with take_files."
            );
        }
    }
    if !left.over.is_empty() {
        let limits = handed::limits(state);
        let _ = write!(
            text,
            " Not attached ({}), over the attachment limits of {} a file and {} together: {}.",
            left.over.len(),
            super::stop::bytes(limits.per_file),
            super::stop::bytes(limits.per_workspace),
            listed(&left.over)
        );
    }
    if !left.unread.is_empty() {
        let _ = write!(
            text,
            " Couldn't be read ({}): {}.",
            left.unread.len(),
            listed(&left.unread)
        );
    }
    if !(left.over.is_empty() && left.unread.is_empty()) {
        text.push_str(" They stay in its own task.");
    }
}

/// At most [`NAMED`] paths, and how many more.
fn listed(paths: &[String]) -> String {
    let mut named = paths
        .iter()
        .take(NAMED)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if paths.len() > NAMED {
        let _ = std::fmt::Write::write_fmt(
            &mut named,
            format_args!(" and {} more", paths.len() - NAMED),
        );
    }
    named
}

/// Directories a hand-back never walks: the runtime's own `.tqa` (its tool-output spill and fetched copies, which the
/// files API hides; a hand-back named 28 of them in a live check before 17.0), a repository's own store, and build or
/// package caches, whose files a command changes as a side effect (the peer review: `git` in a copy would attach
/// `.git/index`).
const SKIPPED: [&str; 6] = [
    ".tqa",
    ".git",
    "node_modules",
    "target",
    "__pycache__",
    ".venv",
];

/// The regular files of `root` modified after `since`, by path, with their sizes, and whether the walk stopped at
/// [`SCANNED`] files: links are never followed, and [`SKIPPED`] directories are never entered.
async fn changed(
    root: std::path::PathBuf,
    since: time::OffsetDateTime,
) -> Result<(Vec<(String, u64)>, bool), GatewayError> {
    let since = std::time::SystemTime::from(since);
    tokio::task::spawn_blocking(move || {
        let mut walk = Walk {
            root: root.clone(),
            since,
            seen: 0,
            found: Vec::new(),
        };
        let mut pending = vec![root];
        while let Some(directory) = pending.pop() {
            pending.extend(walk.directory(&directory));
        }
        walk.found.sort();
        (walk.found, walk.seen >= SCANNED)
    })
    .await
    .map_err(|_| GatewayError::Io)
}

/// Whether a directory is one of [`SKIPPED`].
fn skipped(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| SKIPPED.contains(&name))
}

/// One walk of a copy for [`changed`].
struct Walk {
    root: std::path::PathBuf,
    since: std::time::SystemTime,
    seen: usize,
    found: Vec<(String, u64)>,
}

impl Walk {
    /// Notes the changed files of `directory` and answers its subdirectories.
    fn directory(&mut self, directory: &Path) -> Vec<std::path::PathBuf> {
        let mut below = Vec::new();
        let Ok(entries) = std::fs::read_dir(directory) else {
            return below;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = path.symlink_metadata() else {
                continue;
            };
            if meta.file_type().is_dir() && !skipped(&entry.file_name()) {
                below.push(path);
            } else if meta.file_type().is_file() && self.seen < SCANNED {
                self.seen += 1;
                self.file(&path, &meta);
            }
        }
        below
    }

    fn file(&mut self, path: &Path, meta: &std::fs::Metadata) {
        if !meta.modified().is_ok_and(|modified| modified > self.since) {
            return;
        }
        if let Some(relative) = path.strip_prefix(&self.root).ok().and_then(Path::to_str) {
            self.found.push((relative.to_owned(), meta.len()));
        }
    }
}
