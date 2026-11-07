use super::EventStore;
use crate::GatewayError;
use tqa_types::{ConversationId, Seq, ServerFrame};
use uuid::Uuid;

struct Record {
    seq: i64,
    frame: Option<serde_json::Value>,
    blob_key: Option<String>,
}

impl EventStore {
    pub(crate) async fn watermark(
        &self,
        conversation: &ConversationId,
    ) -> Result<Seq, GatewayError> {
        let id = crate::conversations::uuid(conversation)?;
        crate::io::bounded(self.timeout, &self.cancel, async {
            let last =
                sqlx::query_scalar!("SELECT MAX(seq) FROM events WHERE conversation_id=$1", id)
                    .fetch_one(&self.pool)
                    .await?
                    .unwrap_or(0);
            crate::wire::from_db_sequence(last)
        })
        .await
    }

    pub(crate) async fn read_through(
        &self,
        conversation: &ConversationId,
        after: Seq,
        watermark: i64,
        limit: u32,
    ) -> Result<Vec<ServerFrame>, GatewayError> {
        if !(1..=self.page_cap).contains(&limit) {
            return Err(GatewayError::BadRequest(
                "event page limit is out of bounds",
            ));
        }
        let id = crate::conversations::uuid(conversation)?;
        let after = crate::wire::db_sequence(after)?;
        crate::io::bounded(self.timeout, &self.cancel, async {
            let rows = sqlx::query_as!(Record, "SELECT seq,frame,blob_key FROM events WHERE conversation_id=$1 AND seq>$2 AND seq<=$3 ORDER BY seq LIMIT $4", id, after, watermark, i64::from(limit)).fetch_all(&self.pool).await?;
            let mut result = Vec::with_capacity(rows.len());
            for record in rows { result.push(self.decode(id, record).await?); }
            Ok(result)
        }).await
    }

    /// Whether `run` is a run of this conversation: its `run.started` or `run.resumed` is stored (the lifecycle index).
    ///
    /// # Errors
    /// A store failure.
    pub(crate) async fn has_run(
        &self,
        conversation: Uuid,
        run: &str,
    ) -> Result<bool, GatewayError> {
        crate::io::bounded(self.timeout, &self.cancel, async {
            Ok(sqlx::query_scalar!(
                "SELECT EXISTS(SELECT 1 FROM events WHERE conversation_id=$1 AND lifecycle IN ('run.started','run.resumed') AND run_id=$2) AS \"found!\"",
                conversation,
                run
            )
            .fetch_one(&self.pool)
            .await?)
        })
        .await
    }

    async fn decode(
        &self,
        conversation: Uuid,
        record: Record,
    ) -> Result<ServerFrame, GatewayError> {
        let bytes = match (record.frame, record.blob_key) {
            (Some(value), None) => {
                serde_json::to_vec(&value).map_err(|_| GatewayError::StoreUnavailable)?
            }
            (None, Some(key)) => {
                self.storage
                    .read_blob(conversation, record.seq, &key)
                    .await?
            }
            _ => return Err(GatewayError::StoreUnavailable),
        };
        let frame = ServerFrame::parse(&bytes).map_err(|_| GatewayError::StoreUnavailable)?;
        if crate::wire::sequence(&frame) != Some(crate::wire::from_db_sequence(record.seq)?) {
            return Err(GatewayError::StoreUnavailable);
        }
        Ok(frame)
    }
}
