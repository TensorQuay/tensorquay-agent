//! Recording a screen notice: stored one row, then sent once as a `screen.notice` frame on the task's stream.
use super::notices::Item;
use crate::{GatewayError, app::State, auth::User};
use tqa_types::{
    ConversationId, ScreenEnded, ScreenNoticeKind, ScreenNoticeReason, ScreenPerson, ServerFrame,
};
use uuid::Uuid;

/// What happened, before it is stored: a hand-back says how long it lasted and how it ended, a failed sign-out why.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Notice {
    TakenOver,
    HandedBack { seconds: u64, ended: ScreenEnded },
    SignedOut,
    SignOutFailed(ScreenNoticeReason),
}

impl Notice {
    const fn kind(self) -> ScreenNoticeKind {
        match self {
            Self::TakenOver => ScreenNoticeKind::TakenOver,
            Self::HandedBack { .. } => ScreenNoticeKind::HandedBack,
            Self::SignedOut => ScreenNoticeKind::SignedOut,
            Self::SignOutFailed(_) => ScreenNoticeKind::SignOutFailed,
        }
    }

    const fn reason(self) -> Option<ScreenNoticeReason> {
        match self {
            Self::SignOutFailed(reason) => Some(reason),
            Self::TakenOver | Self::HandedBack { .. } | Self::SignedOut => None,
        }
    }
}

const fn kind_name(kind: ScreenNoticeKind) -> &'static str {
    match kind {
        ScreenNoticeKind::TakenOver => "screen.taken_over",
        ScreenNoticeKind::HandedBack => "screen.handed_back",
        ScreenNoticeKind::SignedOut => "screen.signed_out",
        ScreenNoticeKind::SignOutFailed => "screen.sign_out_failed",
    }
}

pub(super) const fn reason_name(reason: ScreenNoticeReason) -> &'static str {
    match reason {
        ScreenNoticeReason::NotCleared => "not_cleared",
        ScreenNoticeReason::Unconfirmed => "unconfirmed",
    }
}

const fn ended_name(ended: ScreenEnded) -> &'static str {
    match ended {
        ScreenEnded::HandBack => "hand_back",
        ScreenEnded::SocketClosed => "socket_closed",
        ScreenEnded::Restart => "restart",
    }
}

/// Stores `notice` for `conversation`, by `user`, after the conversation's last stored event, and sends it once as a
/// `screen.notice` frame on the task's stream.
///
/// # Errors
/// A store failure, or a sequence past the store's range.
pub(crate) async fn record(
    state: &State,
    conversation: &ConversationId,
    user: &User,
    notice: Notice,
) -> Result<(), GatewayError> {
    let id = crate::conversations::uuid(conversation)?;
    let after = crate::wire::db_sequence(state.events.watermark(conversation).await?)?;
    let (seconds, ended) = match notice {
        Notice::HandedBack { seconds, ended } => (
            Some(i64::try_from(seconds).map_err(|_| GatewayError::StoreUnavailable)?),
            Some(ended_name(ended)),
        ),
        Notice::TakenOver | Notice::SignedOut | Notice::SignOutFailed(_) => (None, None),
    };
    let at = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "INSERT INTO screen_notices (id,conversation_id,kind,by_user,by_name,after_seq,seconds,ended,reason) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING at",
            Uuid::now_v7(),
            id,
            kind_name(notice.kind()),
            user.id,
            user.email,
            after,
            seconds,
            ended,
            notice.reason().map(reason_name)
        )
        .fetch_one(&state.pool)
        .await?)
    })
    .await?;
    let item = Item {
        kind: notice.kind(),
        by: ScreenPerson {
            id: user.id.to_string(),
            name: user.email.clone(),
        },
        at,
        after_seq: u64::try_from(after).map_err(|_| GatewayError::StoreUnavailable)?,
        seconds: seconds.and_then(|value| u64::try_from(value).ok()),
        ended: match notice {
            Notice::HandedBack { ended, .. } => Some(ended),
            Notice::TakenOver | Notice::SignedOut | Notice::SignOutFailed(_) => None,
        },
        reason: notice.reason(),
    };
    state.events.control(conversation, frame(item)?)
}

/// After a restart, ends each Take over the restart cut short: its holder's socket closed with the old process, and
/// the held turns start again, so a hand-back (`ended: restart`, with no length) is recorded for every task whose latest
/// Take over has none after it. Awaited before the gateway listens, so a page that reconnects reads it at once.
///
/// # Errors
/// A store failure.
pub(crate) async fn handed_back_at_start(state: &State) -> Result<usize, GatewayError> {
    let open = crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query!(
            "SELECT conversation_id, by_user, by_name \
             FROM (SELECT DISTINCT ON (conversation_id) conversation_id, kind, by_user, by_name FROM screen_notices \
                   WHERE kind IN ('screen.taken_over', 'screen.handed_back') \
                   ORDER BY conversation_id, at DESC, id DESC) latest \
             WHERE kind = 'screen.taken_over'"
        )
        .fetch_all(&state.pool)
        .await?)
    })
    .await?;
    let mut ended = 0;
    for row in open {
        let conversation: ConversationId = row
            .conversation_id
            .to_string()
            .parse()
            .map_err(|_| GatewayError::StoreUnavailable)?;
        let after = crate::wire::db_sequence(state.events.watermark(&conversation).await?)?;
        crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            sqlx::query!(
                "INSERT INTO screen_notices (id,conversation_id,kind,by_user,by_name,after_seq,ended) \
                 VALUES ($1,$2,'screen.handed_back',$3,$4,$5,'restart')",
                Uuid::now_v7(),
                row.conversation_id,
                row.by_user,
                row.by_name,
                after
            )
            .execute(&state.pool)
            .await?;
            Ok(())
        })
        .await?;
        ended += 1;
    }
    Ok(ended)
}

fn frame(item: Item) -> Result<ServerFrame, GatewayError> {
    Ok(ServerFrame::ScreenNotice {
        kind: item.kind,
        by: item.by,
        at: item
            .at
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| GatewayError::StoreUnavailable)?,
        after_seq: item.after_seq,
        seconds: item.seconds,
        ended: item.ended,
        reason: item.reason,
    })
}
