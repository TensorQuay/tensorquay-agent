//! Task digests (ADR-0041 section 3): when a person's run completes, the gateway writes what the task did into the
//! owner's private folder of the workspace, `.tqa/memory/tasks/<owner>/<date>-<task>.md`: the title, the date, the
//! files the task changed and the first lines of the run's answer. No model call. The runtime lists a person's own
//! digests in the memory index of their later runs. Directory workspaces only (a microVM install writes none until
//! its workspace link gains the operation); nothing here holds a run back.
use super::endpoints::blocking;
use super::path;
use crate::{GatewayError, app::State};
use std::{fmt::Write as _, os::fd::AsFd, sync::Arc, time::Duration};
use tqa_tools::{builtin::memory_note, workspace_tree::memory};
use tqa_types::{ConversationId, Event, ServerFrame};
use uuid::Uuid;

#[cfg(test)]
#[path = "digest_tests.rs"]
mod tests;

/// The whole step's bound: a digest is a convenience and never worth a long wait.
const DEADLINE: Duration = Duration::from_secs(30);
/// At most this many changed files are named; the rest are counted.
const MAX_FILES: usize = 20;
/// The answer's first lines: at most this many characters and lines.
const MAX_ANSWER_CHARS: usize = 600;
const MAX_ANSWER_LINES: usize = 8;
/// The answer is read from at most this many stored frames of the run's last step.
const FRAMES_PER_READ: u32 = 200;
const MAX_READS: usize = 30;

/// What a digest says.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct Digest<'a> {
    pub title: &'a str,
    /// The day the run ended, `YYYY-MM-DD`.
    pub date: &'a str,
    /// The files the task changed, in path order; `None` when they could not be compared.
    pub files: Option<&'a [String]>,
    pub answer: &'a str,
}

/// The digest's file name: the task's first day and its whole id (ids made in the same minute share their first
/// characters), so a task keeps one file however often it runs.
pub(super) fn name(created: time::Date, task: Uuid) -> String {
    format!(
        "{:04}-{:02}-{:02}-{task}.md",
        created.year(),
        u8::from(created.month()),
        created.day()
    )
}

/// One line of display text: no control characters, nothing that opens markup at its start.
fn line(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    flat.trim()
        .trim_start_matches(['#', '>', '-', '*', ' '])
        .to_owned()
}

/// The first lines of an answer, cut at [`MAX_ANSWER_CHARS`] and [`MAX_ANSWER_LINES`].
pub(super) fn first_lines(answer: &str) -> String {
    let mut out = String::new();
    for text in answer
        .lines()
        .map(str::trim_end)
        .filter(|text| !text.trim().is_empty())
        .take(MAX_ANSWER_LINES)
    {
        let room = MAX_ANSWER_CHARS.saturating_sub(out.chars().count());
        if room == 0 {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        let cut: String = text.chars().take(room).collect();
        let whole = cut.len() == text.len();
        out.push_str(&cut);
        if !whole {
            out.push('…');
            break;
        }
    }
    out
}

/// The digest's text. Whatever looks like a key is left out and said so: a digest is listed in later runs.
pub(super) fn compose(digest: &Digest<'_>) -> String {
    let title = line(digest.title);
    let title = if title.is_empty() {
        "(untitled task)".to_owned()
    } else if memory_note::holds_key(&title) {
        "(title left out: it holds what looks like a key)".to_owned()
    } else {
        title
    };
    let mut out = format!("# {title}\ndate: {}\n", digest.date);
    if let Some(files) = digest.files {
        let named: Vec<&str> = files
            .iter()
            .map(String::as_str)
            .filter(|file| !memory_note::holds_key(file))
            .take(MAX_FILES)
            .collect();
        if named.is_empty() {
            out.push_str("files: none changed\n");
        } else {
            out.push_str("files: ");
            out.push_str(&named.join(", "));
            if files.len() > named.len() {
                let _ = write!(out, " (and {} more)", files.len() - named.len());
            }
            out.push('\n');
        }
    }
    let answer = first_lines(digest.answer);
    if memory_note::holds_key(&answer) {
        out.push_str("\n(answer left out: it holds what looks like a key)\n");
    } else if !answer.is_empty() {
        out.push('\n');
        out.push_str(&answer);
        out.push('\n');
    }
    out
}

/// The same digest under a new title: its first line replaced.
pub(super) fn retitle(digest: &str, title: &str) -> String {
    let rest = digest.split_once('\n').map_or("", |(_, rest)| rest);
    let renamed = compose(&Digest {
        title,
        date: "",
        files: None,
        answer: "",
    });
    let first = renamed.lines().next().unwrap_or("# (untitled task)");
    format!("{first}\n{rest}")
}

/// What one stored frame adds to the answer being read: text, or a new attempt of the step, which starts it again
/// (a cut and retried stream must not count twice).
pub(super) fn answer_step(answer: &mut String, frame: &ServerFrame) {
    let ServerFrame::Event { event, .. } = frame else {
        return;
    };
    match event {
        Event::StepStarted { .. } => answer.clear(),
        Event::TextDelta { text } => answer.push_str(text),
        _ => {}
    }
}

/// Whether the frame before a run's start says the gateway woke it (a team's wake) or a lead briefed it: such a run
/// is not a person's task ending.
pub(super) fn woken(opener: Option<&ServerFrame>) -> bool {
    matches!(
        opener,
        Some(ServerFrame::Event {
            event: Event::MessageUser {
                team_wake: Some(_),
                ..
            } | Event::MessageUser {
                lead_brief: Some(_),
                ..
            },
            ..
        })
    )
}

/// A run of the task completed: writes its digest in the background when long-term memory is on.
pub(crate) fn after_run(state: &Arc<State>, id: &ConversationId) {
    if !state.config.memory.enabled || state.config.uses_block_images() {
        return;
    }
    let (state, id) = (Arc::clone(state), id.clone());
    let tasks = state.tasks.clone();
    tasks.spawn(async move {
        let written = tokio::select! {biased;
            () = state.cancel.cancelled() => Ok(()),
            written = tokio::time::timeout(DEADLINE, write(&state, &id)) => written.unwrap_or(Err(GatewayError::Timeout)),
        };
        if let Err(error) = written {
            tracing::warn!(conversation_id = %id, code = error.code(), "the task's memory digest was not written");
        }
    });
}

/// The task has a new title (the model named it, or its person renamed it): its digest's first line follows. Never
/// an error for the caller: a digest that kept its old title is logged, and the next run rewrites it.
pub(crate) async fn title_changed(state: &State, id: &ConversationId, title: &str) {
    if !state.config.memory.enabled || state.config.uses_block_images() {
        return;
    }
    let renamed = tokio::time::timeout(DEADLINE, rename(state, id, title.to_owned())).await;
    if let Ok(Err(error)) | Err(error) = renamed.map_err(|_| GatewayError::Timeout) {
        tracing::warn!(conversation_id = %id, code = error.code(), "the task's memory digest kept its old title");
    }
}

/// The task as the digest needs it.
struct Task {
    title: String,
    owner: Uuid,
    created: time::Date,
    workspace: Uuid,
}

async fn task(state: &State, conversation: Uuid) -> Result<Option<Task>, GatewayError> {
    let row = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT c.title, c.owner_id, c.created_at, \
             EXISTS (SELECT 1 FROM agent_team_members m WHERE m.conversation_id=c.id) AS \"helper!\" \
             FROM conversations c WHERE c.id=$1 AND c.state='active'",
            conversation
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    // A team's helper is a part of its lead's task, not a task of the person's own.
    let Some(row) = row.filter(|row| !row.helper) else {
        return Ok(None);
    };
    let workspace = crate::io::bounded(
        state.config.io_timeout(),
        &state.cancel,
        crate::workspaces::of_conversation(&state.pool, conversation),
    )
    .await?
    .id;
    Ok(Some(Task {
        title: row.title,
        owner: row.owner_id,
        created: row.created_at.date(),
        workspace,
    }))
}

async fn write(state: &State, id: &ConversationId) -> Result<(), GatewayError> {
    let conversation = crate::conversations::uuid(id)?;
    let Some(task) = task(state, conversation).await? else {
        return Ok(());
    };
    let marks = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT r.started AS \"started!\", \
             EXISTS (SELECT 1 FROM events e WHERE e.conversation_id=$1 AND e.seq>r.started AND e.call_id IS NOT NULL) AS \"tools!\", \
             (SELECT MIN(e.seq) FROM events e WHERE e.conversation_id=$1 AND e.seq>r.started \
               AND e.step=(SELECT MAX(s.step) FROM events s WHERE s.conversation_id=$1 AND s.seq>r.started)) AS answer_from \
             FROM (SELECT MAX(seq) AS started FROM events WHERE conversation_id=$1 AND lifecycle='run.started') r \
             WHERE r.started IS NOT NULL",
            conversation
        )
        .fetch_optional(&state.pool)
        .await?)
    })
    .await?;
    let Some(marks) = marks else {
        return Ok(());
    };
    // The message that opened the run sits just before its start.
    let opener = state
        .events
        .read(id, u64::try_from(marks.started - 2).unwrap_or(0).into(), 1)
        .await?;
    if woken(opener.first()) {
        return Ok(());
    }
    let root = state.storage.workspace(task.workspace).await?;
    let (owner, file) = (task.owner.to_string(), name(task.created, conversation));
    // A run that used no tool (a thank-you, a question about the work) leaves the task's digest as it is.
    if !marks.tools {
        let (root, owner, file) = (root.clone(), owner.clone(), file.clone());
        let kept = blocking(move || {
            let root = path::open_root(&root)?;
            let tasks = memory::tasks_directory(root.as_fd(), &owner)?;
            Ok(memory::read_digest(tasks.as_fd(), &file)?.is_some())
        })
        .await?;
        if kept {
            return Ok(());
        }
    }
    let answer = answer(state, id, marks.answer_from).await?;
    // The files the task changed since its first recorded message; without a record, the digest goes without them.
    let files = match super::changes::task_files(state, task.workspace, conversation).await {
        Ok(files) => files,
        Err(error) => {
            tracing::warn!(conversation_id = %id, code = error.code(), "the digest lists no files: the comparison failed");
            None
        }
    };
    let today = time::OffsetDateTime::now_utc().date();
    let date = format!(
        "{:04}-{:02}-{:02}",
        today.year(),
        u8::from(today.month()),
        today.day()
    );
    let text = compose(&Digest {
        title: &task.title,
        date: &date,
        files: files.as_deref(),
        answer: &answer,
    });
    blocking(move || {
        let root = path::open_root(&root)?;
        let tasks = memory::tasks_directory(root.as_fd(), &owner)?;
        Ok(memory::write_digest(tasks.as_fd(), &file, &text)?)
    })
    .await
}

/// The text of the run's last step, from its selected attempt.
async fn answer(
    state: &State,
    id: &ConversationId,
    from: Option<i64>,
) -> Result<String, GatewayError> {
    let Some(from) = from else {
        return Ok(String::new());
    };
    let mut after = u64::try_from(from - 1).unwrap_or(0);
    let mut answer = String::new();
    for _ in 0..MAX_READS {
        let frames = state.events.read(id, after.into(), FRAMES_PER_READ).await?;
        let count = frames.len();
        for frame in &frames {
            answer_step(&mut answer, frame);
            if let Some(seq) = crate::wire::sequence(frame)
                .and_then(|seq| serde_json::to_value(seq).ok())
                .and_then(|seq| seq.as_u64())
            {
                after = seq;
            }
        }
        if count < usize::try_from(FRAMES_PER_READ).unwrap_or(usize::MAX) {
            break;
        }
    }
    Ok(answer)
}

async fn rename(state: &State, id: &ConversationId, title: String) -> Result<(), GatewayError> {
    let conversation = crate::conversations::uuid(id)?;
    let Some(task) = task(state, conversation).await? else {
        return Ok(());
    };
    let root = state.storage.workspace(task.workspace).await?;
    let (owner, file) = (task.owner.to_string(), name(task.created, conversation));
    blocking(move || {
        let root = path::open_root(&root)?;
        let tasks = memory::tasks_directory(root.as_fd(), &owner)?;
        let Some(digest) = memory::read_digest(tasks.as_fd(), &file)? else {
            return Ok(());
        };
        Ok(memory::write_digest(
            tasks.as_fd(),
            &file,
            &retitle(&digest, &title),
        )?)
    })
    .await
}
