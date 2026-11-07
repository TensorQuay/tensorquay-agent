//! GW-I-052 (ADR-0028): a seed entry applies only when the seed file's value changed since it was last imported, so a
//! rotation made through the store survives a restart and an operator who edits the file still wins.

#[path = "support/fixture.rs"]
mod support;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{os::unix::fs::PermissionsExt, path::Path};
use support::TestResult;
use tokio_util::sync::CancellationToken;
use tqa_gateway::{BuiltinSecretStore, SecretFiles, SecretStore, SecretStoreConfig};

/// Opens the store as a start would, with `seed` as the private seed file's `[secrets]` table (or no file).
async fn start(
    pool: &PgPool,
    directory: &Path,
    seed: Option<&str>,
) -> Result<BuiltinSecretStore, Box<dyn std::error::Error>> {
    let path = directory.join("secrets.toml");
    if let Some(entries) = seed {
        std::fs::write(&path, format!("[secrets]\n{entries}"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    let files = SecretFiles::new(directory.join("install.key"), seed.map(|_| path))?;
    Ok(BuiltinSecretStore::open(
        pool.clone(),
        files,
        SecretStoreConfig::default(),
        CancellationToken::new(),
    )
    .await?)
}

async fn value(
    store: &BuiltinSecretStore,
    reference: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let secret = store
        .get(&reference.parse()?)
        .await?
        .ok_or("missing secret")?;
    Ok(secret.expose_secret().to_owned())
}

/// The `version`, `last_set_by` and `seed_state` of one reference.
async fn provenance(
    pool: &PgPool,
    reference: &str,
) -> Result<(i64, String, Option<String>), Box<dyn std::error::Error>> {
    let row = sqlx::query("SELECT version,last_set_by,seed_state FROM secrets WHERE ref=$1")
        .bind(reference)
        .fetch_one(pool)
        .await?;
    Ok((row.try_get(0)?, row.try_get(1)?, row.try_get(2)?))
}

/// `actor_kind`, `action`, `before_version`, `after_version`, `value_hash_changed`.
type AuditRow = (String, String, Option<i64>, Option<i64>, bool);

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_052_an_unchanged_seed_keeps_a_rotation_and_a_changed_seed_wins(
    pool: PgPool,
) -> TestResult {
    support::require_database()?;
    let directory = tempfile::tempdir()?;
    let reference = "secrets/route";
    let store = start(
        &pool,
        directory.path(),
        Some("\"secrets/route\" = \"seed-one\"\n"),
    )
    .await?;
    assert_eq!(value(&store, reference).await?, "seed-one");
    assert_eq!(
        provenance(&pool, reference).await?,
        (1, "seed".into(), Some("current".into()))
    );
    store
        .put(&reference.parse()?, SecretString::from("rotated"))
        .await?;
    assert_eq!(
        provenance(&pool, reference).await?,
        (2, "admin".into(), Some("current".into()))
    );
    // The same seed again: the rotation survives the restart.
    let store = start(
        &pool,
        directory.path(),
        Some("\"secrets/route\" = \"seed-one\"\n"),
    )
    .await?;
    assert_eq!(value(&store, reference).await?, "rotated");
    assert_eq!(provenance(&pool, reference).await?.0, 2);
    // The operator edits the file: the seed wins, the version moves, and an audit row records it without a value.
    let store = start(
        &pool,
        directory.path(),
        Some("\"secrets/route\" = \"seed-two\"\n"),
    )
    .await?;
    assert_eq!(value(&store, reference).await?, "seed-two");
    assert_eq!(
        provenance(&pool, reference).await?,
        (3, "seed".into(), Some("current".into()))
    );
    let audit = sqlx::query(
        "SELECT actor_kind,action,before_version,after_version,value_hash_changed FROM admin_audit \
         WHERE area='secrets' AND item=$1 ORDER BY at, id",
    )
    .bind(reference)
    .fetch_all(&pool)
    .await?;
    let rows: Vec<AuditRow> = audit
        .iter()
        .map(|row| {
            Ok((
                row.try_get(0)?,
                row.try_get(1)?,
                row.try_get(2)?,
                row.try_get(3)?,
                row.try_get(4)?,
            ))
        })
        .collect::<Result<_, sqlx::Error>>()?;
    assert_eq!(
        rows,
        vec![
            ("seed".into(), "create".into(), None, Some(1), true),
            ("seed".into(), "replace".into(), Some(2), Some(3), true),
        ]
    );
    let text: String =
        sqlx::query_scalar("SELECT string_agg(admin_audit::text, ' ') FROM admin_audit")
            .fetch_one(&pool)
            .await?;
    for secret in ["seed-one", "seed-two", "rotated"] {
        assert!(!text.contains(secret));
    }
    Ok(())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_052_removed_seed_items_are_kept_and_seed_digests_are_keyed(
    pool: PgPool,
) -> TestResult {
    support::require_database()?;
    let directory = tempfile::tempdir()?;
    let seed = "\"secrets/kept\" = \"alpha\"\n\"secrets/dropped\" = \"beta\"\n";
    start(&pool, directory.path(), Some(seed)).await?;
    let digest: Vec<u8> =
        sqlx::query_scalar("SELECT seed_digest FROM secrets WHERE ref='secrets/kept'")
            .fetch_one(&pool)
            .await?;
    assert_eq!(digest.len(), 32);
    assert_ne!(
        digest,
        Sha256::digest(b"alpha").to_vec(),
        "the digest must be keyed"
    );
    let store = start(
        &pool,
        directory.path(),
        Some("\"secrets/kept\" = \"alpha\"\n"),
    )
    .await?;
    assert_eq!(value(&store, "secrets/dropped").await?, "beta");
    assert_eq!(
        provenance(&pool, "secrets/dropped").await?.2.as_deref(),
        Some("removed")
    );
    assert_eq!(
        provenance(&pool, "secrets/kept").await?.2.as_deref(),
        Some("current")
    );
    // Without a seed file every seeded item is no longer seeded; a reference never seeded has no seed state.
    let store = start(&pool, directory.path(), None).await?;
    store
        .put(&"secrets/manual".parse()?, SecretString::from("gamma"))
        .await?;
    assert_eq!(
        provenance(&pool, "secrets/kept").await?.2.as_deref(),
        Some("removed")
    );
    assert_eq!(
        provenance(&pool, "secrets/manual").await?,
        (1, "admin".into(), None)
    );
    Ok(())
}
