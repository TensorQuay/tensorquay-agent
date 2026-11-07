use super::{FakeSecretStore, SecretRef, SecretStore, SecretStoreConfig, crypto::Cipher};
use secrecy::{ExposeSecret, SecretSlice};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// TEST: GW-I-015
#[test]
fn gw_i_029_references_and_bounds_reject_ambiguous_or_unbounded_values() -> TestResult {
    for name in [
        "",
        "local",
        "secrets/",
        "secrets/../key",
        "secrets/a/b",
        "secrets/é",
        "secrets/a?b",
    ] {
        assert!(name.parse::<SecretRef>().is_err());
    }
    assert!(
        format!("secrets/{}", "x".repeat(121))
            .parse::<SecretRef>()
            .is_err()
    );
    assert_eq!(
        "secrets/local_1-a".parse::<SecretRef>()?.as_str(),
        "secrets/local_1-a"
    );
    for source in [
        "io_timeout=0",
        "value_bytes=0",
        "seed_bytes=0",
        "references=0",
        "unknown=1",
    ] {
        let config = toml::from_str::<SecretStoreConfig>(source);
        assert!(config.is_err() || config.is_ok_and(|config| config.validate().is_err()));
    }
    assert!(SecretStoreConfig::default().value("").is_err());
    assert!(
        SecretStoreConfig::default()
            .value(&"x".repeat(8193))
            .is_err()
    );
    Ok(())
}

// TEST: GW-I-015
#[tokio::test]
async fn gw_i_029_fake_rotates_and_fails_closed_during_an_outage() -> TestResult {
    let store = FakeSecretStore::default();
    let reference = "secrets/local".parse()?;
    assert!(store.get(&reference).await?.is_none());
    store.put(&reference, "synthetic-first".into()).await?;
    store.put(&reference, "synthetic-next".into()).await?;
    assert_eq!(
        store
            .get(&reference)
            .await?
            .ok_or("missing rotation")?
            .expose_secret(),
        "synthetic-next"
    );
    assert_eq!(store.refs().await?, vec![reference.clone()]);
    store.set_unavailable(true);
    assert_eq!(
        store
            .get(&reference)
            .await
            .err()
            .ok_or("outage ignored")?
            .code(),
        "secret_unavailable"
    );
    store.set_unavailable(false);
    store.delete(&reference).await?;
    store.delete(&reference).await?;
    assert!(store.refs().await?.is_empty());
    Ok(())
}

// TEST: GW-I-015
#[test]
fn gw_i_029_authenticated_encryption_binds_the_reference_and_rejects_corruption() -> TestResult {
    let key: SecretSlice<u8> = vec![7; 32].into();
    let cipher = Cipher::new(&key)?;
    let reference = "secrets/local".parse()?;
    let value = "synthetic-value".into();
    let (nonce, mut bytes) = cipher.encrypt(&reference, &value, &SecretStoreConfig::default())?;
    assert_eq!(
        cipher.decrypt(&reference, &nonce, &bytes)?.expose_secret(),
        "synthetic-value"
    );
    assert!(
        cipher
            .decrypt(&"secrets/other".parse()?, &nonce, &bytes)
            .is_err()
    );
    bytes[0] ^= 1;
    assert!(cipher.decrypt(&reference, &nonce, &bytes).is_err());
    assert!(cipher.decrypt(&reference, &nonce[..23], &bytes).is_err());
    assert!(Cipher::new(&vec![7; 31].into()).is_err());
    Ok(())
}
