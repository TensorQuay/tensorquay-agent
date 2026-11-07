//! Seed tombstones (ADR-0028): a seeded item an administrator deleted stays deleted across restarts. Deleting a seeded
//! item records the keyed digest of the seed value last imported for it; the seed import skips the item while its
//! digest still matches, imports it again once the file's value changes, and forgets the tombstone once the file no
//! longer lists the item. Re-creating the item through the API takes the tombstone's digest over, so an unchanged seed
//! does not overwrite the new record either.
use crate::GatewayError;
use sqlx::PgConnection;
use subtle::ConstantTimeEq;

/// Records that a seeded item was deleted; `digest` is its row's seed digest (`None` for an item never seeded, which
/// needs no tombstone).
///
/// # Errors
/// Storage failure.
pub(crate) async fn bury(
    connection: &mut PgConnection,
    area: &str,
    item: &str,
    digest: Option<&[u8]>,
) -> Result<(), GatewayError> {
    let Some(digest) = digest else {
        return Ok(());
    };
    sqlx::query!(
        "INSERT INTO seed_tombstones (area,item,seed_digest) VALUES ($1,$2,$3) \
         ON CONFLICT (area,item) DO UPDATE SET seed_digest=EXCLUDED.seed_digest, buried_at=CURRENT_TIMESTAMP",
        area,
        item,
        digest
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Removes the item's tombstone and returns its digest, for an item re-created through the API.
///
/// # Errors
/// Storage failure.
pub(crate) async fn unearth(
    connection: &mut PgConnection,
    area: &str,
    item: &str,
) -> Result<Option<Vec<u8>>, GatewayError> {
    Ok(sqlx::query_scalar!(
        "DELETE FROM seed_tombstones WHERE area=$1 AND item=$2 RETURNING seed_digest",
        area,
        item
    )
    .fetch_optional(connection)
    .await?)
}

/// Whether the seed import must skip a missing item: its tombstone holds the same digest. A tombstone with another
/// digest is removed, since the file's value changed and the item is imported again.
///
/// # Errors
/// Storage failure.
pub(crate) async fn buried(
    connection: &mut PgConnection,
    area: &str,
    item: &str,
    digest: &[u8],
) -> Result<bool, GatewayError> {
    let Some(stored) = sqlx::query_scalar!(
        "SELECT seed_digest FROM seed_tombstones WHERE area=$1 AND item=$2 FOR UPDATE",
        area,
        item
    )
    .fetch_optional(&mut *connection)
    .await?
    else {
        return Ok(false);
    };
    if bool::from(stored.ct_eq(digest)) {
        return Ok(true);
    }
    unearth(connection, area, item).await?;
    Ok(false)
}

/// Forgets the tombstones of items the seed no longer lists, so listing one again later counts as a new entry.
///
/// # Errors
/// Storage failure.
pub(crate) async fn forget_unlisted(
    connection: &mut PgConnection,
    area: &str,
    listed: &[String],
) -> Result<(), GatewayError> {
    sqlx::query!(
        "DELETE FROM seed_tombstones WHERE area=$1 AND NOT (item = ANY($2))",
        area,
        listed
    )
    .execute(connection)
    .await?;
    Ok(())
}
