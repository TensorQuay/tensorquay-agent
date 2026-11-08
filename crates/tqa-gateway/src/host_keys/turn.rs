//! A turn's keys (ADR-0037 §2, §3): as the gateway delivers a turn to the runtime, it chooses the run's keys and mints
//! their placeholders; only the delivered frame carries them, never a table.
use super::{placeholders, run_keys};
use crate::{GatewayError, egress::AllowList};
use secrecy::ExposeSecret as _;
use tqa_types::HostKeyGrant;
use uuid::Uuid;

/// The keys for a turn delivered to `sandbox` of `conversation`, whose messages came from `senders`: none unless they
/// all came from one person (fail closed), who is active. `delivery` names the turn, so an undelivered one can drop its
/// placeholders ([`placeholders::drop_delivery`]).
///
/// # Errors
/// Storage failure, or no randomness.
pub(crate) async fn for_turn(
    pool: &sqlx::PgPool,
    (conversation, sandbox): (Uuid, &str),
    senders: &[Uuid],
    (network, egress_disabled): (&AllowList, bool),
    delivery: Uuid,
) -> Result<Vec<HostKeyGrant>, GatewayError> {
    let mut transaction = pool.begin().await?;
    let sender = match senders.split_first() {
        Some((&sender, rest)) if rest.iter().all(|other| *other == sender) => sender,
        _ => {
            tracing::warn!(conversation_id = %conversation, "a turn from more than one person gets no host keys");
            placeholders::supersede(&mut transaction, conversation).await?;
            transaction.commit().await?;
            return Ok(Vec::new());
        }
    };
    let (active, candidates) = run_keys::load(&mut transaction, sender).await?;
    let keys = run_keys::choose(active, candidates, network, egress_disabled, None);
    let minted = placeholders::mint(
        &mut transaction,
        (conversation, sandbox, sender),
        delivery,
        &keys,
    )
    .await?;
    transaction.commit().await?;
    grants(minted)
}

/// Whether the turn's one sender has keys the task could use: for a runtime too old to take them, which gets a notice
/// instead (ADR-0037, guardrails §8). None are minted.
///
/// # Errors
/// Storage failure.
pub(crate) async fn would_have_keys(
    pool: &sqlx::PgPool,
    senders: &[Uuid],
    (network, egress_disabled): (&AllowList, bool),
) -> Result<bool, GatewayError> {
    let sender = match senders.split_first() {
        Some((&sender, rest)) if rest.iter().all(|other| *other == sender) => sender,
        _ => return Ok(false),
    };
    let mut connection = pool.acquire().await?;
    let (active, candidates) = run_keys::load(&mut connection, sender).await?;
    Ok(!run_keys::choose(active, candidates, network, egress_disabled, None).is_empty())
}

/// The keys of the run going in the task, newly minted for `sandbox`, whose runtime just registered: the run gets them
/// back when it resumes (GW-I-165). Only keys it was given that still exist, still allowed and still the sender's.
///
/// # Errors
/// Storage failure, or no randomness.
pub(crate) async fn for_resume(
    pool: &sqlx::PgPool,
    (conversation, sandbox): (Uuid, &str),
    (network, egress_disabled): (&AllowList, bool),
) -> Result<Vec<HostKeyGrant>, GatewayError> {
    let mut transaction = pool.begin().await?;
    let Some((run, sender, given)) =
        placeholders::in_progress(&mut transaction, conversation).await?
    else {
        return Ok(Vec::new());
    };
    let (active, candidates) = run_keys::load(&mut transaction, sender).await?;
    let given = given.into_iter().collect();
    let keys = run_keys::choose(active, candidates, network, egress_disabled, Some(&given));
    let minted = placeholders::mint_for_run(
        &mut transaction,
        (conversation, sandbox, sender),
        &run,
        &keys,
    )
    .await?;
    transaction.commit().await?;
    grants(minted)
}

fn grants(
    minted: Vec<(run_keys::RunKey, secrecy::SecretString)>,
) -> Result<Vec<HostKeyGrant>, GatewayError> {
    minted
        .into_iter()
        .map(|(key, placeholder)| {
            HostKeyGrant::new(
                key.name,
                (key.host, key.port, key.path_prefix),
                (key.scheme, key.user_name),
                placeholder.expose_secret().to_owned(),
            )
            .map_err(|_| GatewayError::StoreUnavailable)
        })
        .collect()
}
