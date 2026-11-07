//! GW-I-029: encrypted storage, rotation and authenticated decryption without plaintext disclosure.

#[path = "support/fixture.rs"]
mod support;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use support::TestResult;
use tokio_util::sync::CancellationToken;
use tqa_gateway::{BuiltinSecretStore, SecretFiles, SecretStore, SecretStoreConfig};

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_029_ciphertexts_rotate_without_restart_and_delete_idempotently(
    pool: PgPool,
) -> TestResult {
    support::require_database()?;
    let directory = tempfile::tempdir()?;
    let files = SecretFiles::new(directory.path().join("install.key"), None)?;
    let store = BuiltinSecretStore::open(
        pool.clone(),
        files.clone(),
        SecretStoreConfig::default(),
        CancellationToken::new(),
    )
    .await?;
    let reference = "secrets/local".parse()?;
    assert!(store.get(&reference).await?.is_none());
    let value = SecretString::from(format!("synthetic-{}", uuid::Uuid::now_v7()));
    store.put(&reference, value.clone()).await?;
    assert_eq!(
        store
            .get(&reference)
            .await?
            .ok_or("missing secret")?
            .expose_secret(),
        value.expose_secret()
    );
    let first = sqlx::query!(
        "SELECT nonce,ciphertext FROM secrets WHERE ref=$1",
        reference.as_str()
    )
    .fetch_one(&pool)
    .await?;
    assert!(
        !first
            .ciphertext
            .windows(value.expose_secret().len())
            .any(|bytes| bytes == value.expose_secret().as_bytes())
    );
    let rotation = SecretString::from(format!("rotated-{}", uuid::Uuid::now_v7()));
    store.put(&reference, rotation.clone()).await?;
    let second = sqlx::query!(
        "SELECT nonce,ciphertext FROM secrets WHERE ref=$1",
        reference.as_str()
    )
    .fetch_one(&pool)
    .await?;
    assert_ne!(first.nonce, second.nonce);
    assert_ne!(first.ciphertext, second.ciphertext);
    assert_eq!(
        store
            .get(&reference)
            .await?
            .ok_or("missing rotation")?
            .expose_secret(),
        rotation.expose_secret()
    );
    let reopened = BuiltinSecretStore::open(
        pool,
        files,
        SecretStoreConfig::default(),
        CancellationToken::new(),
    )
    .await?;
    assert_eq!(
        reopened
            .get(&reference)
            .await?
            .ok_or("missing persisted value")?
            .expose_secret(),
        rotation.expose_secret()
    );
    assert_eq!(store.refs().await?, vec![reference.clone()]);
    store.delete(&reference).await?;
    store.delete(&reference).await?;
    assert!(reopened.get(&reference).await?.is_none());
    assert!(store.refs().await?.is_empty());
    Ok(())
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_029_wrong_key_and_swapped_ciphertext_fail_authentication(pool: PgPool) -> TestResult {
    support::require_database()?;
    let directory = tempfile::tempdir()?;
    let original = BuiltinSecretStore::open(
        pool.clone(),
        SecretFiles::new(directory.path().join("original.key"), None)?,
        SecretStoreConfig::default(),
        CancellationToken::new(),
    )
    .await?;
    let wrong = BuiltinSecretStore::open(
        pool.clone(),
        SecretFiles::new(directory.path().join("different.key"), None)?,
        SecretStoreConfig::default(),
        CancellationToken::new(),
    )
    .await?;
    let first = "secrets/first".parse()?;
    let second = "secrets/second".parse()?;
    let value = SecretString::from(format!("synthetic-{}", uuid::Uuid::now_v7()));
    original.put(&first, value.clone()).await?;
    original
        .put(&second, "another-synthetic-value".into())
        .await?;
    let error = wrong.get(&first).await.err().ok_or("wrong key decrypted")?;
    assert_eq!(error.code(), "secret_unavailable");
    assert!(!format!("{error:?} {error}").contains(value.expose_secret()));
    sqlx::query!("UPDATE secrets SET nonce=(SELECT nonce FROM secrets WHERE ref=$1),ciphertext=(SELECT ciphertext FROM secrets WHERE ref=$1) WHERE ref=$2", first.as_str(), second.as_str()).execute(&pool).await?;
    assert!(original.get(&second).await.is_err());
    Ok(())
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_029_seed_permissions_values_and_store_bounds_fail_closed(pool: PgPool) -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    support::require_database()?;
    let directory = tempfile::tempdir()?;
    let seed = directory.path().join("seed.toml");
    tokio::fs::write(
        &seed,
        "[secrets]\n\"secrets/seeded\"='synthetic-seed-value'\n",
    )
    .await?;
    tokio::fs::set_permissions(&seed, std::fs::Permissions::from_mode(0o644)).await?;
    let files = SecretFiles::new(directory.path().join("install.key"), Some(seed.clone()))?;
    assert!(
        BuiltinSecretStore::open(
            pool.clone(),
            files.clone(),
            SecretStoreConfig::default(),
            CancellationToken::new()
        )
        .await
        .is_err()
    );
    tokio::fs::set_permissions(&seed, std::fs::Permissions::from_mode(0o600)).await?;
    let store = BuiltinSecretStore::open(
        pool.clone(),
        files.clone(),
        SecretStoreConfig::default(),
        CancellationToken::new(),
    )
    .await?;
    assert_eq!(
        store
            .get(&"secrets/seeded".parse()?)
            .await?
            .ok_or("missing seed")?
            .expose_secret(),
        "synthetic-seed-value"
    );
    assert!(
        store
            .put(&"secrets/large".parse()?, "x".repeat(8193).into())
            .await
            .is_err()
    );
    tokio::fs::write(&seed, "[unknown]\nsecret='never-echo-this'\n").await?;
    let error = BuiltinSecretStore::open(
        pool,
        files,
        SecretStoreConfig::default(),
        CancellationToken::new(),
    )
    .await
    .err()
    .ok_or("unknown seed table accepted")?;
    assert!(!format!("{error:?} {error}").contains("never-echo-this"));
    Ok(())
}

#[path = "support/log_capture.rs"]
mod log_capture;

/// One global capture for the process. A subscriber scoped to one test's future can miss events: other tests in this
/// process hit the same log call sites at the same time, and tracing caches a call site's interest globally.
fn capture_logs() -> Result<log_capture::Capture, Box<dyn std::error::Error>> {
    static CAPTURE: std::sync::OnceLock<Option<log_capture::Capture>> = std::sync::OnceLock::new();
    CAPTURE
        .get_or_init(|| {
            let capture = log_capture::Capture::default();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(capture.clone())
                .with_ansi(false)
                .with_max_level(tracing::Level::TRACE)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .ok()
                .map(|()| capture)
        })
        .clone()
        .ok_or_else(|| "another global tracing subscriber was already set".into())
}

// TEST: GW-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_029_decrypted_values_never_appear_in_logs_or_errors(pool: PgPool) -> TestResult {
    support::require_database()?;
    let directory = tempfile::tempdir()?;
    let capture = capture_logs()?;
    let value = SecretString::from(format!("synthetic-sensitive-{}", uuid::Uuid::now_v7()));
    async {
        let store = BuiltinSecretStore::open(
            pool,
            SecretFiles::new(directory.path().join("install.key"), None)?,
            SecretStoreConfig::default(),
            CancellationToken::new(),
        )
        .await?;
        let reference = "secrets/log-fixture".parse()?;
        store.put(&reference, value.clone()).await?;
        assert_eq!(
            store
                .get(&reference)
                .await?
                .ok_or("missing secret")?
                .expose_secret(),
            value.expose_secret()
        );
        assert_eq!(store.refs().await?, vec![reference]);
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await?;
    let output = capture.text()?;
    assert!(output.contains("secrets/log-fixture"));
    assert!(output.contains("count=1"));
    assert!(!output.contains(value.expose_secret()));
    Ok(())
}
