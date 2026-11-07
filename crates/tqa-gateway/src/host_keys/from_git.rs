//! Git credentials become host keys (ADR-0037 §7): at its start the gateway copies each Git credential not yet copied
//! into its owner's own host key, `basic` with its user or `bearer`, for its remote's host and path, its value sealed
//! again under a host key's reference, and `check_name` set so its owner confirms the name it was given. The second
//! release removes each original once copied (migration 0088 removed those an earlier release had copied), so an install
//! upgrading from before the copy existed still gets its keys at the first start. One that can't be copied (its remote
//! isn't a key's address, or its owner holds the most keys) is kept, unused, and counted for the administrator's status
//! page ([`kept`]); the tables go in the next release.
use super::{api, rules};
use crate::{GatewayError, secrets::SecretStore};
use sqlx::PgPool;
use uuid::Uuid;

/// Credentials read per query; a start reads them all, in batches.
const BATCH: i64 = 1000;

/// The name for a remote: its host's first label and its path's segments, upper case, in the name pattern; a name
/// that would be reserved or start with a digit gets a `REPO_` prefix; at most 64 characters.
pub(super) fn base_name(address: &rules::Address) -> String {
    let words = std::iter::once(address.host.split('.').next().unwrap_or_default())
        .chain(
            address
                .path_prefix
                .as_deref()
                .unwrap_or_default()
                .split('/')
                .map(|segment| segment.strip_suffix(".git").unwrap_or(segment)),
        )
        .map(|word| {
            // The address keeps a path percent-encoded: decode it so an escape's digits aren't taken for words.
            percent_decoded(word)
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() {
                        character.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
        })
        .flat_map(|word| {
            word.split('_')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("_");
    let mut named = if words.is_empty() {
        "REPO".to_owned()
    } else {
        words
    };
    if rules::name(&named).is_err() {
        named = format!("REPO_{named}");
    }
    named.truncate(64);
    named.trim_end_matches('_').to_owned()
}

/// `text` with its `%XX` escapes decoded, lossily.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = |at: usize| {
            bytes
                .get(at)
                .and_then(|byte| char::from(*byte).to_digit(16))
        };
        if bytes[index] == b'%'
            && let (Some(high), Some(low)) = (hex(index + 1), hex(index + 2))
        {
            decoded.push(u8::try_from(high * 16 + low).unwrap_or_default());
            index += 3;
            continue;
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// `base`, or for the `n`th try `base_n`, within 64 characters.
pub(super) fn numbered(base: &str, n: u32) -> String {
    if n <= 1 {
        return base.to_owned();
    }
    let suffix = format!("_{n}");
    let mut head = base.to_owned();
    head.truncate(64 - suffix.len());
    format!("{}{suffix}", head.trim_end_matches('_'))
}

/// Copies the Git credentials not yet copied into host keys, removing each original once its key is committed; how
/// many it copied. A credential whose value is gone has nothing to copy and is removed. One whose remote isn't a host
/// key's address, or whose owner holds the most keys, is kept and logged ([`kept`] counts them). An original copied
/// before a stop that left it behind is removed first. Stops at the first store failure; the next start goes on.
///
/// # Errors
/// A store or secret-store failure.
pub(crate) async fn copy_git_credentials(
    pool: &PgPool,
    store: &dyn SecretStore,
) -> Result<u64, GatewayError> {
    remove_copied(pool, store).await?;
    let mut copied = 0;
    let mut after: Option<(Uuid, String)> = None;
    loop {
        let (after_user, after_remote) = after.clone().unzip();
        let rows = sqlx::query!(
            "SELECT user_id, remote, kind, username, secret_ref FROM git_credentials WHERE moved_at IS NULL \
             AND ($1::uuid IS NULL OR (user_id, remote) > ($1, $2)) ORDER BY user_id, remote LIMIT $3",
            after_user,
            after_remote,
            BATCH
        )
        .fetch_all(pool)
        .await?;
        let Some(last) = rows.last() else {
            break;
        };
        after = Some((last.user_id, last.remote.clone()));
        for row in rows {
            let original = Original {
                user: row.user_id,
                remote: row.remote,
                secret_ref: row.secret_ref,
            };
            let user_name = (row.kind == "basic").then_some(row.username).flatten();
            if copy_one(pool, store, &original, user_name).await? {
                copied += 1;
            }
        }
    }
    if copied > 0 {
        tracing::info!(
            copied,
            "Git credentials copied into host keys and their originals removed"
        );
    }
    Ok(copied)
}

/// Copies one credential into its owner's key and removes the original; whether it was copied. One whose value is
/// gone is removed; one that can't become a key is kept and logged. A `basic` credential has its user name.
async fn copy_one(
    pool: &PgPool,
    store: &dyn SecretStore,
    original: &Original,
    user_name: Option<String>,
) -> Result<bool, GatewayError> {
    let Ok(address) = rules::address(&original.remote) else {
        tracing::warn!(user_id = %original.user, "a Git credential's remote isn't a key's address; kept, unused");
        return Ok(false);
    };
    let Some(value) = store.get(&original.secret_ref.parse()?).await? else {
        original.remove(pool, store).await?;
        return Ok(false);
    };
    let mut transaction = pool.begin().await?;
    sqlx::query!("SELECT id FROM users WHERE id=$1 FOR UPDATE", original.user)
        .fetch_one(&mut *transaction)
        .await?;
    let held = sqlx::query_scalar!(
        "SELECT count(*) AS \"n!\" FROM host_keys WHERE owner_id=$1",
        original.user
    )
    .fetch_one(&mut *transaction)
    .await?;
    if held >= api::OWN_LIMIT {
        tracing::warn!(user_id = %original.user, "a Git credential wasn't copied: its owner holds the most keys");
        return Ok(false);
    }
    let name = free_name(&mut transaction, original.user, &base_name(&address)).await?;
    let reference = api::new_reference()?;
    store.put(&reference, value).await?;
    let scheme = if user_name.is_some() {
        "basic"
    } else {
        "bearer"
    };
    sqlx::query!(
        "INSERT INTO host_keys (id, owner_id, name, host, port, path_prefix, scheme, user_name, secret_ref, \
         check_name) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,true)",
        Uuid::now_v7(),
        original.user,
        name,
        address.host,
        i32::from(address.port),
        address.path_prefix,
        scheme,
        user_name,
        reference.as_str()
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query!(
        "UPDATE git_credentials SET moved_at=CURRENT_TIMESTAMP WHERE user_id=$1 AND remote=$2",
        original.user,
        original.remote
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    original.remove(pool, store).await?;
    Ok(true)
}

/// A Git credential's original: its row and its sealed value.
struct Original {
    user: Uuid,
    remote: String,
    secret_ref: String,
}

impl Original {
    /// Removes the sealed value, then the row, so a stop between the two leaves a row the next start removes again.
    async fn remove(&self, pool: &PgPool, store: &dyn SecretStore) -> Result<(), GatewayError> {
        store.delete(&self.secret_ref.parse()?).await?;
        sqlx::query!(
            "DELETE FROM git_credentials WHERE user_id=$1 AND remote=$2",
            self.user,
            self.remote
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}

/// Removes the originals already copied whose removal a stop interrupted.
async fn remove_copied(pool: &PgPool, store: &dyn SecretStore) -> Result<(), GatewayError> {
    loop {
        let rows = sqlx::query!(
            "SELECT user_id, remote, secret_ref FROM git_credentials WHERE moved_at IS NOT NULL LIMIT $1",
            BATCH
        )
        .fetch_all(pool)
        .await?;
        if rows.is_empty() {
            return Ok(());
        }
        for row in rows {
            Original {
                user: row.user_id,
                remote: row.remote,
                secret_ref: row.secret_ref,
            }
            .remove(pool, store)
            .await?;
        }
    }
}

/// The Git credentials kept because they couldn't become keys, by why: for the administrator's status page.
#[derive(Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Kept {
    /// Their remote isn't a web address a key can have: an administrator or their owner adds the key by hand.
    pub not_web_address: u64,
    /// Their owner held the most keys at the last start: copied at a start after the owner frees one.
    pub waiting: u64,
}

/// Counts the Git credentials not copied, by why (at most [`BATCH`] times ten, read in batches).
///
/// # Errors
/// A store failure.
pub(crate) async fn kept(pool: &PgPool) -> Result<Kept, GatewayError> {
    let remotes = sqlx::query_scalar!(
        "SELECT remote FROM git_credentials WHERE moved_at IS NULL LIMIT $1",
        BATCH * 10
    )
    .fetch_all(pool)
    .await?;
    let mut kept = Kept::default();
    for remote in remotes {
        if rules::address(&remote).is_ok() {
            kept.waiting += 1;
        } else {
            kept.not_web_address += 1;
        }
    }
    Ok(kept)
}

/// The first free numbering of `base` among the owner's own keys and the shared keys they may use.
async fn free_name(
    connection: &mut sqlx::PgConnection,
    user: Uuid,
    base: &str,
) -> Result<String, GatewayError> {
    for n in 1..=999 {
        let candidate = numbered(base, n);
        if api::name_holder(connection, user, &candidate, None)
            .await?
            .is_none()
        {
            return Ok(candidate);
        }
    }
    Err(GatewayError::Conflict("name_taken"))
}

#[cfg(test)]
#[path = "from_git_tests.rs"]
mod tests;
