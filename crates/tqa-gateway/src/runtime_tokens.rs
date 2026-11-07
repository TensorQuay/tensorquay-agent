//! Internal sandbox-instance credentials. Plaintext is never part of an HTTP response.

use crate::{GatewayError, auth::token, link::LinkRegistry};
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tqa_types::ConversationId;

mod reservations;
pub use reservations::ReservationId;

/// A bounded backend instance identifier; accepts container and future microVM identifiers.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SandboxId(String);

impl SandboxId {
    /// Borrows the validated identifier without allocating.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for SandboxId {
    type Err = GatewayError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if !(1..=128).contains(&value.len())
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(GatewayError::BadRequest("invalid sandbox identifier"));
        }
        Ok(Self(value.into()))
    }
}

/// Trusted manager API for hashed, revocable per-instance credentials; never exposed as an HTTP route.
#[derive(Clone)]
pub struct RuntimeTokens {
    pub(crate) pool: PgPool,
    pub(crate) links: Arc<LinkRegistry>,
    pub(crate) cancel: CancellationToken,
    pub(crate) timeout: Duration,
    pub(crate) reservation_secs: u32,
}

impl RuntimeTokens {
    pub(crate) async fn register(
        &self,
        plaintext: &SecretString,
        peer: IpAddr,
        revision: u16,
    ) -> Result<(ConversationId, crate::link::Registration), GatewayError> {
        let hash = token::credential(plaintext.expose_secret())?;
        let peer = peer.to_string();
        crate::io::bounded(self.timeout, &self.cancel, async {
            let mut transaction = self.pool.begin().await?;
            // Hold a database row lock through registration so revoke/delete cannot finish
            // between credential validation and installing the corresponding live link.
            let row = sqlx::query!("SELECT t.conversation_id,t.sandbox_id AS \"sandbox_id!\" FROM runtime_tokens t JOIN conversations c ON c.id=t.conversation_id WHERE t.token_hash=$1 AND t.peer_address=$2::text::inet AND t.sandbox_id IS NOT NULL AND (t.expires_at IS NULL OR t.expires_at>CURRENT_TIMESTAMP) AND c.state='active' FOR SHARE OF t,c", hash, peer)
                .fetch_optional(&mut *transaction).await?.ok_or(GatewayError::Unauthorized)?;
            let id: ConversationId = row.conversation_id.to_string().parse().map_err(|_| GatewayError::StoreUnavailable)?;
            let registration = self.links.register_at(id.clone(), row.sandbox_id.parse()?, revision)?;
            transaction.commit().await?;
            Ok((id, registration))
        }).await
    }

    /// Mints 256 random bits for a new sandbox instance, replacing a previous instance's credential.
    ///
    /// # Errors
    /// Rejects deleted conversations, reused instance identifiers and unavailable storage.
    pub async fn mint(
        &self,
        conversation: &ConversationId,
        sandbox: &SandboxId,
        peer: IpAddr,
    ) -> Result<SecretString, GatewayError> {
        let id = crate::conversations::uuid(conversation)?;
        let plaintext = token::mint()?;
        let hash = token::digest(plaintext.expose_secret());
        let previous = crate::io::bounded(self.timeout, &self.cancel, async {
            let mut transaction = self.pool.begin().await?;
            let previous =
                reservations::activate(&mut transaction, id, &hash, sandbox, peer).await?;
            transaction.commit().await?;
            Ok(previous)
        })
        .await?;
        self.remove_previous(conversation, previous)?;
        Ok(plaintext)
    }

    fn remove_previous(
        &self,
        conversation: &ConversationId,
        previous: Option<String>,
    ) -> Result<(), GatewayError> {
        if let Some(previous) = previous {
            self.links
                .remove_sandbox(conversation, &previous.parse()?)?;
        }
        Ok(())
    }

    /// Deletes a sandbox's credential and cancels its matching runtime link; missing rows succeed.
    ///
    /// # Errors
    /// Returns a bounded store failure if revocation cannot be confirmed.
    pub async fn revoke(&self, sandbox: &SandboxId) -> Result<(), GatewayError> {
        let conversation = crate::io::bounded(self.timeout, &self.cancel, async {
            let mut transaction = self.pool.begin().await?;
            let conversation = sqlx::query_scalar!(
                "DELETE FROM runtime_tokens WHERE sandbox_id=$1 RETURNING conversation_id",
                sandbox.as_str()
            )
            .fetch_optional(&mut *transaction)
            .await?;
            // Its placeholders end with the credential (ADR-0037 §3).
            crate::host_keys::end_sandbox(&mut transaction, sandbox.as_str()).await?;
            transaction.commit().await?;
            Ok::<_, GatewayError>(conversation)
        })
        .await?;
        if let Some(id) = conversation {
            self.links.remove_sandbox(
                &id.to_string()
                    .parse()
                    .map_err(|_| GatewayError::StoreUnavailable)?,
                sandbox,
            )?;
        }
        Ok(())
    }

    /// Resolves an active credential to its conversation and sandbox binding.
    ///
    /// Only the exact sandbox address can authenticate. Invalid, expired, revoked and legacy
    /// unbound credentials return `None`; forwarded headers never supply this address.
    ///
    /// # Errors
    /// Returns a bounded store failure when credential status cannot be determined.
    pub async fn resolve(
        &self,
        plaintext: &SecretString,
        peer: IpAddr,
    ) -> Result<Option<(ConversationId, SandboxId)>, GatewayError> {
        let Ok(hash) = token::credential(plaintext.expose_secret()) else {
            return Ok(None);
        };
        let peer = peer.to_string();
        crate::io::bounded(self.timeout, &self.cancel, async {
            let row = sqlx::query!("SELECT t.conversation_id,t.sandbox_id AS \"sandbox_id!\" FROM runtime_tokens t JOIN conversations c ON c.id=t.conversation_id WHERE t.token_hash=$1 AND t.peer_address=$2::text::inet AND t.sandbox_id IS NOT NULL AND (t.expires_at IS NULL OR t.expires_at>CURRENT_TIMESTAMP) AND c.state='active'", hash, peer)
                .fetch_optional(&self.pool).await?;
            row.map(|row| Ok((row.conversation_id.to_string().parse().map_err(|_| GatewayError::StoreUnavailable)?, row.sandbox_id.parse()?))).transpose()
        }).await
    }
}

fn token_write_error(error: &sqlx::Error) -> GatewayError {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        GatewayError::Conflict("sandbox_token_exists")
    } else {
        GatewayError::StoreUnavailable
    }
}
