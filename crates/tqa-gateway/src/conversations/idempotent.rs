//! Conversation writes share the actions' owner/key namespace, lock and canonical body fingerprint.
use crate::{GatewayError, app::State, auth::token};
use axum::http::{HeaderMap, StatusCode};
use serde_json::Value;
use sha2::Digest;
use sqlx::PgConnection;
use uuid::Uuid;

/// One idempotent write: its owner, key, exact route and request fingerprint, and the task its stored answer is about.
pub(crate) struct Keyed<'a> {
    owner: Uuid,
    key: std::borrow::Cow<'a, str>,
    route: String,
    fingerprint: [u8; 32],
    about: Option<Uuid>,
}

impl<'a> Keyed<'a> {
    /// Reads the required key and fingerprints the typed request body.
    ///
    /// # Errors
    /// Rejects a missing or malformed key and a body that cannot be serialised.
    pub(crate) fn new(
        owner: Uuid,
        headers: &'a HeaderMap,
        route: String,
        body: &impl serde::Serialize,
    ) -> Result<Self, GatewayError> {
        let key = crate::actions::idempotency_key(headers)?;
        let fingerprint = sha2::Sha256::digest(serde_json::to_vec(body)?).into();
        Ok(Self {
            owner,
            key: key.into(),
            route,
            fingerprint,
            about: None,
        })
    }

    /// Like [`Keyed::new`], for a run's write: the key is scoped to the run's conversation, so two conversations of
    /// one owner may send the same key (both first runs, calls numbered per turn) and each keeps its own record. The
    /// stored key is a digest of the conversation and the caller's key, within the table's bound.
    ///
    /// # Errors
    /// As [`Keyed::new`].
    pub(crate) fn for_conversation(
        owner: Uuid,
        conversation: Uuid,
        headers: &'a HeaderMap,
        route: String,
        body: &impl serde::Serialize,
    ) -> Result<Self, GatewayError> {
        let mut keyed = Self::new(owner, headers, route, body)?;
        let digest = token::digest(&format!("{conversation}:{}", keyed.key));
        keyed.key = format!("conversation:{}", crate::hex::hex(&digest)).into();
        keyed.about = Some(conversation);
        Ok(keyed)
    }

    /// Serialises writes with this key, purges an expired record and returns a cached response when one exists.
    ///
    /// # Errors
    /// A key used on another route conflicts; a key reused with another body is refused.
    pub(crate) async fn begin(
        &self,
        state: &State,
        connection: &mut PgConnection,
    ) -> Result<Option<(StatusCode, Value)>, GatewayError> {
        let hash = token::digest(&format!("{}:{}", self.owner, self.key));
        let lock = i64::from_be_bytes(hash[..8].try_into().map_err(|_| GatewayError::Io)?);
        let retention = f64::from(state.config.limits.idempotency_hours) * 3600.0;
        crate::db::queue(&mut *connection, crate::db::Queue::Key(lock)).await?;
        sqlx::query!("DELETE FROM idempotency WHERE key=$1 AND user_id=$2 AND execution_status<>'queued' AND created_at<=CURRENT_TIMESTAMP-make_interval(secs=>$3::double precision)", self.key.as_ref(), self.owner, retention).execute(&mut *connection).await?;
        let cached = sqlx::query!("SELECT route,response_status,response_body,body_hash,execution_status FROM idempotency WHERE key=$1 AND user_id=$2", self.key.as_ref(), self.owner)
            .fetch_optional(&mut *connection).await?;
        let Some(cached) = cached else {
            return Ok(None);
        };
        if cached.route != self.route {
            return Err(GatewayError::Conflict("idempotency_route"));
        }
        if cached.body_hash.as_deref() != Some(self.fingerprint.as_slice()) {
            return Err(GatewayError::IdempotencyKeyReused);
        }
        let status = u16::try_from(cached.response_status)
            .ok()
            .and_then(|value| StatusCode::from_u16(value).ok())
            .ok_or(GatewayError::StoreUnavailable)?;
        let body = serde_json::from_slice(&cached.response_body)
            .map_err(|_| GatewayError::StoreUnavailable)?;
        Ok(Some((status, body)))
    }

    /// Records the first response in the same transaction as the write it describes.
    ///
    /// # Errors
    /// Fails when the record cannot be stored; the caller's transaction then rolls back.
    pub(crate) async fn record(
        &self,
        connection: &mut PgConnection,
        status: StatusCode,
        body: &Value,
    ) -> Result<(), GatewayError> {
        self.insert(connection, self.about, status, body).await
    }

    /// Like [`Keyed::record`], for an answer about task `conversation` (its create, rename or environment change): the
    /// answer holds its title or description, so it names the task in `about_conversation` and goes with the task's
    /// purge. The task's row must exist already, in the caller's transaction.
    ///
    /// # Errors
    /// As [`Keyed::record`].
    pub(crate) async fn record_about(
        &self,
        connection: &mut PgConnection,
        conversation: Uuid,
        status: StatusCode,
        body: &Value,
    ) -> Result<(), GatewayError> {
        self.insert(connection, Some(conversation), status, body)
            .await
    }

    async fn insert(
        &self,
        connection: &mut PgConnection,
        about: Option<Uuid>,
        status: StatusCode,
        body: &Value,
    ) -> Result<(), GatewayError> {
        sqlx::query!("INSERT INTO idempotency (key,user_id,route,response_status,response_body,body_hash,about_conversation) VALUES ($1,$2,$3,$4,$5,$6,$7)", self.key.as_ref(), self.owner, self.route, i16::try_from(status.as_u16()).map_err(|_| GatewayError::Io)?, serde_json::to_vec(body)?, self.fingerprint.as_slice(), about).execute(&mut *connection).await?;
        Ok(())
    }
}
