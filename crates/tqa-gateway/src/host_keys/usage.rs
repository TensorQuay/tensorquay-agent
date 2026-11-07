//! A keyed request's audit (ADR-0037 §8): every use of a key writes its `secret_reads` row, `consumer_kind = 'host'`
//! and `consumer = host:port`, attributed to the caller's task, run and person. A value is read only once its row is
//! committed, and a use whose row can't be written is refused (`unrecorded`): nothing reaches the host.
use super::use_rule::Outcome;
use crate::{
    app::State,
    secrets::reads::{self, ConsumerKind, ReadContext},
};
use secrecy::SecretString;

/// One use of a key: its value's store reference, where the request goes, and who made it.
pub(crate) struct Use<'a> {
    pub(crate) reference: &'a str,
    pub(crate) host: &'a str,
    pub(crate) port: u16,
    pub(crate) context: ReadContext,
}

impl Use<'_> {
    fn consumer(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// The key's value for the swap, once its `ok` row is committed; otherwise why not, and nothing is sent.
pub(crate) async fn value(state: &State, key_use: &Use<'_>) -> Result<SecretString, Outcome> {
    let reference = key_use.reference.parse().map_err(|_| Outcome::Revoked)?;
    match reads::read(
        state,
        &reference,
        (ConsumerKind::Host, &key_use.consumer()),
        &key_use.context,
    )
    .await
    {
        Ok(Some(value)) => Ok(value),
        // Deleted between the proxy's check and the read.
        Ok(None) => Err(Outcome::Revoked),
        Err(crate::GatewayError::KeyUseUnrecorded) => Err(Outcome::Unrecorded),
        Err(_) => Err(Outcome::Unavailable),
    }
}

/// Records a refused use; the request is refused whether or not the row could be queued.
pub(crate) async fn refuse(state: &State, key_use: &Use<'_>, outcome: Outcome) {
    let Ok(reference) = key_use.reference.parse() else {
        return;
    };
    if !reads::refused(
        state,
        &reference,
        &key_use.consumer(),
        &key_use.context,
        outcome.as_str(),
    )
    .await
    {
        tracing::warn!(
            outcome = outcome.as_str(),
            "a refused host key use could not be recorded"
        );
    }
}
