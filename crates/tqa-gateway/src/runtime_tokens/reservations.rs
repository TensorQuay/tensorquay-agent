use super::{RuntimeTokens, SandboxId, token, token_write_error};
use crate::GatewayError;
use secrecy::{ExposeSecret, SecretString};
use sqlx::{Postgres, Transaction};
use std::net::IpAddr;
use tqa_types::ConversationId;
use uuid::Uuid;

/// Opaque, expiring bootstrap reservation. Activation consumes its database record exactly once.
#[derive(Debug)]
pub struct ReservationId(Uuid);

impl RuntimeTokens {
    /// Persists an inert credential hash, with the configured bootstrap expiry.
    ///
    /// # Errors
    /// Rejects deleted conversations, another live reservation and unavailable storage.
    pub async fn reserve(
        &self,
        conversation: &ConversationId,
    ) -> Result<(ReservationId, SecretString), GatewayError> {
        let id = crate::conversations::uuid(conversation)?;
        let reservation = ReservationId(Uuid::now_v7());
        let plaintext = token::mint()?;
        let hash = token::digest(plaintext.expose_secret());
        let lifetime = f64::from(self.reservation_secs);
        crate::io::bounded(self.timeout, &self.cancel, async {
            let mut transaction = self.pool.begin().await?;
            active_conversation(&mut transaction, id).await?;
            sqlx::query!("DELETE FROM runtime_token_reservations WHERE conversation_id=$1 AND expires_at<=CURRENT_TIMESTAMP", id).execute(&mut *transaction).await?;
            sqlx::query!("INSERT INTO runtime_token_reservations (id,conversation_id,token_hash,expires_at) VALUES ($1,$2,$3,CURRENT_TIMESTAMP+make_interval(secs=>$4::double precision))", reservation.0,id,hash,lifetime)
                .execute(&mut *transaction).await.map_err(|error| token_write_error(&error))?;
            transaction.commit().await?;
            Ok(())
        }).await?;
        Ok((reservation, plaintext))
    }

    /// Activates an unexpired reservation for the actual backend identity and transport address.
    ///
    /// # Errors
    /// Rejects consumed, expired or invalid reservations, reused instances and deleted conversations.
    pub async fn bind(
        &self,
        reservation: &ReservationId,
        sandbox: &SandboxId,
        peer: IpAddr,
    ) -> Result<(), GatewayError> {
        let (conversation, previous) = crate::io::bounded(self.timeout, &self.cancel, async {
            let mut transaction = self.pool.begin().await?;
            let conversation=sqlx::query_scalar!("SELECT conversation_id FROM runtime_token_reservations WHERE id=$1",reservation.0)
                .fetch_optional(&mut *transaction).await?.ok_or(GatewayError::Conflict("runtime_reservation_expired"))?;
            // Use the same lock order as reserve, then check expiry at the activation boundary.
            active_conversation(&mut transaction,conversation).await?;
            let row = sqlx::query!("DELETE FROM runtime_token_reservations WHERE id=$1 AND expires_at>clock_timestamp() RETURNING conversation_id,token_hash", reservation.0)
                .fetch_optional(&mut *transaction).await?.ok_or(GatewayError::Conflict("runtime_reservation_expired"))?;
            let previous = activate(&mut transaction, row.conversation_id, &row.token_hash, sandbox, peer).await?;
            transaction.commit().await?;
            Ok((row.conversation_id, previous))
        }).await?;
        self.remove_previous(
            &conversation
                .to_string()
                .parse()
                .map_err(|_| GatewayError::StoreUnavailable)?,
            previous,
        )
    }

    /// Clears every reservation, at start-up: one can only belong to a create of a process that is gone (a restart
    /// that interrupted it). Left in place, it took its conversation's one reservation until it expired, so the next
    /// start failed `sandbox_token_exists` and its queued turn with it (GW-I-252).
    ///
    /// # Errors
    /// A store failure.
    pub(crate) async fn clear_reservations(&self) -> Result<u64, GatewayError> {
        crate::io::bounded(self.timeout, &self.cancel, async {
            Ok(sqlx::query!("DELETE FROM runtime_token_reservations")
                .execute(&self.pool)
                .await?
                .rows_affected())
        })
        .await
    }

    pub(crate) async fn abandon(&self, reservation: &ReservationId) -> Result<(), GatewayError> {
        crate::io::bounded(self.timeout, &self.cancel, async {
            sqlx::query!(
                "DELETE FROM runtime_token_reservations WHERE id=$1",
                reservation.0
            )
            .execute(&self.pool)
            .await?;
            Ok(())
        })
        .await
    }
}

pub(super) async fn activate(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    hash: &[u8],
    sandbox: &SandboxId,
    peer: IpAddr,
) -> Result<Option<String>, GatewayError> {
    if peer.is_unspecified() || peer.is_multicast() {
        return Err(GatewayError::BadRequest("invalid sandbox address"));
    }
    active_conversation(transaction, id).await?;
    let previous = sqlx::query_scalar!(
        "SELECT sandbox_id FROM runtime_tokens WHERE conversation_id=$1",
        id
    )
    .fetch_optional(&mut **transaction)
    .await?
    .flatten();
    if previous.as_deref() == Some(sandbox.as_str()) {
        return Err(GatewayError::Conflict("sandbox_token_exists"));
    }
    let peer = peer.to_string();
    sqlx::query!("INSERT INTO runtime_tokens (conversation_id,sandbox_id,token_hash,peer_address) VALUES ($1,$2,$3,$4::text::inet) ON CONFLICT (conversation_id) DO UPDATE SET sandbox_id=EXCLUDED.sandbox_id,token_hash=EXCLUDED.token_hash,peer_address=EXCLUDED.peer_address,expires_at=NULL,created_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP", id,sandbox.as_str(),hash,peer)
        .execute(&mut **transaction).await.map_err(|error| token_write_error(&error))?;
    // The replaced sandbox's placeholders end with its credential (ADR-0037 §3).
    if let Some(previous) = previous.as_deref() {
        crate::host_keys::end_sandbox(transaction, previous).await?;
    }
    Ok(previous)
}

async fn active_conversation(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<(), GatewayError> {
    let state = sqlx::query_scalar!("SELECT state FROM conversations WHERE id=$1 FOR UPDATE", id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(GatewayError::NotFound)?;
    if state != "active" {
        return Err(GatewayError::Conflict("conversation_deleted"));
    }
    Ok(())
}
