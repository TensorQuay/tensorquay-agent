//! A woken run's end (ADR-0014's amendment, A5 and A10a, decided on 20 Nov 2026): nothing more is charged to its
//! channel, and a run that failed or was stopped is said there once.
use super::super::store;
use crate::GatewayError;
use sqlx::PgConnection;
use tqa_types::{ChannelNoticeKind, MessageBody};
use uuid::Uuid;

/// How a run ended, as a channel is told (C-6).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ending {
    /// It completed: the channel is told nothing.
    Completed,
    /// It failed, with its failure code when it had one, and the gateway's own refusal when that is what ended it
    /// (`gateway.channel_budget 409`): never any other failure detail, which may be the task's own words.
    Failed {
        /// The run's failure code.
        code: Option<String>,
        /// The gateway's refusal, as the runtime reports it (`gateway.<code> <status>`).
        refusal: Option<String>,
    },
    /// It was stopped.
    Stopped,
}

impl Ending {
    /// How the run that `frame` ends ended, by its lifecycle (`run.completed`, `run.failed` or `run.cancelled`).
    pub(crate) fn of(lifecycle: &str, frame: &tqa_types::ServerFrame) -> Self {
        match (lifecycle, frame) {
            (
                "run.failed",
                tqa_types::ServerFrame::Event {
                    event: tqa_types::Event::RunFailed { code, reason, .. },
                    ..
                },
            ) => Self::Failed {
                code: serde_json::to_value(code)
                    .ok()
                    .and_then(|code| code.as_str().map(str::to_owned)),
                refusal: refusal(reason),
            },
            ("run.failed", _) => Self::Failed {
                code: None,
                refusal: None,
            },
            ("run.cancelled", _) => Self::Stopped,
            _ => Self::Completed,
        }
    }
}

/// `reason` when it is exactly the gateway's refusal as the runtime words it: `gateway.`, a code of lowercase letters
/// and `_`, a space and a three-digit status. Anything else is the run's own detail and stays with the task.
fn refusal(reason: &str) -> Option<String> {
    let (code, status) = reason.strip_prefix("gateway.")?.split_once(' ')?;
    let code_ok = !code.is_empty()
        && code.len() <= 64
        && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    let status_ok = status.len() == 3 && status.bytes().all(|b| b.is_ascii_digit());
    (code_ok && status_ok).then(|| reason.to_owned())
}

/// A run ends on `conversation`: nothing more is charged to a channel (A5). A run a wake started that failed or was
/// stopped is said in that channel, once, while it is open (decided on 20 Nov 2026, C-6); answers the channel and the
/// notice's seq, to publish after the commit.
///
/// # Errors
/// Storage failure.
pub(crate) async fn run_ended(
    connection: &mut PgConnection,
    conversation: Uuid,
    ending: Ending,
) -> Result<Option<(Uuid, u64)>, GatewayError> {
    let mut said = None;
    if ending != Ending::Completed
        && let Some(woken) = sqlx::query!(
            "SELECT g.channel_id, COALESCE(m.name, c.profile->>'display_name', c.profile->>'name', 'An agent') \
             AS \"name!\" FROM channel_charges g JOIN channels ch ON ch.id = g.channel_id AND ch.state = 'open' \
             JOIN conversations c ON c.id = g.conversation_id \
             LEFT JOIN channel_members m ON m.channel_id = g.channel_id AND m.conversation_id = g.conversation_id \
             WHERE g.conversation_id = $1",
            conversation
        )
        .fetch_optional(&mut *connection)
        .await?
    {
        let (kind, code, refusal, text) = match ending {
            Ending::Failed { code, refusal } => (
                ChannelNoticeKind::WokenRunFailed,
                code.clone(),
                refusal,
                format!("{}'s run failed ({}).", woken.name, code.as_deref().unwrap_or("unknown")),
            ),
            _ => (
                ChannelNoticeKind::WokenRunStopped,
                None,
                None,
                format!("{}'s run was stopped.", woken.name),
            ),
        };
        let author = store::gateway_author();
        let message = store::append_body(
            connection,
            woken.channel_id,
            &author,
            (
                None,
                &MessageBody::Notice {
                    kind,
                    member: Some(woken.name),
                    // Named as Members names it, for its owner by its title when it has no name here (C-15).
                    member_id: Some(conversation.to_string()),
                    code,
                    refusal,
                    resets_at: None,
                    task_seq: None,
                    text,
                },
            ),
        )
        .await?;
        said = Some((woken.channel_id, message.seq));
    }
    sqlx::query!(
        "DELETE FROM channel_charges WHERE conversation_id = $1",
        conversation
    )
    .execute(&mut *connection)
    .await?;
    Ok(said)
}
