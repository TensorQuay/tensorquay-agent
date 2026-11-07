//! The team's messages in their read shape (ADR-0034 S1), as the lead's and members' reads and the person's page get
//! them. Split from `store.rs` (G-06).
use super::store::Caller;
use crate::GatewayError;
use sqlx::PgConnection;
use time::format_description::well_known::Rfc3339;
use tqa_types::{FromKind, TeamFrom, TeamMessage, TeamTo, ToKind};

/// The team's messages at `seqs`, in order, in their read shape.
pub(crate) async fn messages(
    connection: &mut PgConnection,
    caller: &Caller,
    seqs: &[i64],
) -> Result<Vec<TeamMessage>, GatewayError> {
    let rows = sqlx::query!(
        "SELECT m.seq, m.author_kind, m.author_id, m.author_name, m.to_kind, t.name AS \"to_name?\", m.body, \
         m.created_at FROM channel_messages m LEFT JOIN agent_team_members t ON t.conversation_id = m.to_conversation \
         WHERE m.channel_id = $1 AND m.seq = ANY($2) ORDER BY m.seq",
        caller.channel,
        seqs
    )
    .fetch_all(&mut *connection)
    .await?;
    let lead = caller.lead.to_string();
    rows.into_iter()
        .map(|row| {
            let from = match row.author_kind.as_str() {
                "conversation" if row.author_id == lead => TeamFrom {
                    kind: FromKind::Lead,
                    name: None,
                },
                "conversation" => TeamFrom {
                    kind: FromKind::Member,
                    name: row.author_name,
                },
                "user" => TeamFrom {
                    kind: FromKind::Person,
                    name: row.author_name,
                },
                _ => TeamFrom {
                    kind: FromKind::Gateway,
                    name: None,
                },
            };
            let kind = match row.to_kind.as_deref() {
                Some("lead") => ToKind::Lead,
                Some("member") => ToKind::Member,
                Some("members") => ToKind::Members,
                Some("team") => ToKind::Team,
                Some("person") => ToKind::Person,
                _ => return Err(GatewayError::StoreUnavailable),
            };
            Ok(TeamMessage {
                seq: u64::try_from(row.seq).map_err(|_| GatewayError::StoreUnavailable)?,
                from,
                to: TeamTo {
                    kind,
                    name: if kind == ToKind::Member {
                        row.to_name
                    } else {
                        None
                    },
                },
                at: row
                    .created_at
                    .format(&Rfc3339)
                    .map_err(|_| GatewayError::StoreUnavailable)?,
                body: serde_json::from_value(row.body)
                    .map_err(|_| GatewayError::StoreUnavailable)?,
            })
        })
        .collect()
}
