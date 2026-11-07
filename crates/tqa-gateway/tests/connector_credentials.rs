//! GW-I-024 (ADR-0031 decision 3): connectors and the vault. A user stores their own credential for a connector and
//! the connector may have a shared one; values are never returned and are sealed at rest; the user's own credential
//! is chosen for their calls and the shared one otherwise; a credential only counts while the connector keeps the
//! origin it was set for, and deleting a connector deletes users' credentials for it.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tqa_gateway::CredentialSource;

const SEED: &str = "[[connectors]]\nname='tracker'\nurl='https://mcp.tracker.example/mcp'\n\
    auth={scheme='bearer'}\nsecret_ref='secrets/tracker-shared'\n\
    [[connectors]]\nname='open'\nurl='https://mcp.open.example/mcp'\n";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn source(f: &Fixture, user: uuid::Uuid) -> Result<Option<(CredentialSource, String)>> {
    Ok(f.gateway
        .connector_credential(user, "tracker")
        .await?
        .map(|credential| {
            (
                credential.source,
                credential.value.expose_secret().to_owned(),
            )
        }))
}

async fn set(f: &Fixture, cookie: &str, name: &str, value: &str) -> Result<u16> {
    let path = format!("/api/v1/connectors/{name}/credential");
    Ok(call(
        f,
        Method::PUT,
        &path,
        cookie,
        (Some(json!({"value": value})), None),
    )
    .await?
    .0)
}

async fn mine(f: &Fixture, cookie: &str) -> Result<Value> {
    let (status, body) = call(f, Method::GET, "/api/v1/connectors", cookie, (None, None)).await?;
    assert_eq!(status, 200, "{body}");
    Ok(body)
}

// TEST: GW-I-024
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_024_the_users_own_credential_is_chosen_and_the_shared_one_otherwise(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, SEED, async |f| {
        let (alice, alice_cookie) = f.admin().await?;
        let (bob, bob_cookie) = f.member("bob@example.invalid").await?;
        assert_eq!(
            source(f, alice.id).await?,
            None,
            "the shared secret has no value yet"
        );
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/admin/secrets/tracker-shared",
            &alice_cookie,
            (Some(json!({"value": "synthetic-shared"})), None),
        )
        .await?;
        assert_eq!(status, 201);
        assert_eq!(
            set(f, &alice_cookie, "tracker", "synthetic-alice").await?,
            204
        );
        assert_eq!(
            source(f, alice.id).await?,
            Some((CredentialSource::User, "synthetic-alice".into()))
        );
        assert_eq!(
            source(f, bob.id).await?,
            Some((CredentialSource::Shared, "synthetic-shared".into()))
        );
        // Listings say whether a credential is held, never what it is; the sealed value is not plaintext at rest.
        let listing = mine(f, &alice_cookie).await?;
        assert_eq!(listing["items"][1]["name"], "tracker");
        assert_eq!(
            (
                &listing["items"][1]["has_credential"],
                &listing["items"][1]["scheme"]
            ),
            (&json!(true), &json!("bearer"))
        );
        assert_eq!(listing["items"][0]["has_credential"], false);
        assert!(!listing.to_string().contains("synthetic"));
        assert_eq!(
            mine(f, &bob_cookie).await?["items"][1]["has_credential"],
            false
        );
        let sealed: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT ciphertext FROM secrets WHERE ref LIKE 'secrets/connector-%'",
        )
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(sealed.len(), 1);
        assert!(
            !sealed[0]
                .windows(15)
                .any(|window| window == b"synthetic-alice")
        );
        // Replace, then delete: the shared credential takes over and the sealed value is gone.
        assert_eq!(
            set(f, &alice_cookie, "tracker", "synthetic-alice-2").await?,
            204
        );
        assert_eq!(
            source(f, alice.id).await?,
            Some((CredentialSource::User, "synthetic-alice-2".into()))
        );
        let (status, _) = call(
            f,
            Method::DELETE,
            "/api/v1/connectors/tracker/credential",
            &alice_cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 204);
        assert_eq!(
            source(f, alice.id).await?,
            Some((CredentialSource::Shared, "synthetic-shared".into()))
        );
        let left: i64 =
            sqlx::query_scalar("SELECT count(*) FROM secrets WHERE ref LIKE 'secrets/connector-%'")
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(left, 0);
        let (status, _) = call(
            f,
            Method::DELETE,
            "/api/v1/connectors/tracker/credential",
            &alice_cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 404);
        Ok(())
    })
    .await
}

// TEST: GW-I-024
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_024_a_credential_follows_its_connectors_origin_and_dies_with_it(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, SEED, async |f| {
        let (alice, cookie) = f.admin().await?;
        assert_eq!(set(f, &cookie, "tracker", "synthetic-alice").await?, 204);
        // Another server behind the same name: the user's credential no longer counts, and none is shared yet.
        let moved = json!({"url": "https://elsewhere.example/mcp", "auth": {"scheme": "bearer"}, "version": 1});
        let (status, body) = call(f, Method::PUT, "/api/v1/admin/connectors/tracker", &cookie, (Some(moved), None)).await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(source(f, alice.id).await?, None);
        assert_eq!(mine(f, &cookie).await?["items"][1]["has_credential"], false);
        // Setting it again binds it to the new origin.
        assert_eq!(set(f, &cookie, "tracker", "synthetic-alice").await?, 204);
        assert_eq!(source(f, alice.id).await?.map(|found| found.0), Some(CredentialSource::User));
        // Deleting the connector deletes the users' credentials for it and their sealed values.
        let (status, _) = call(f, Method::DELETE, "/api/v1/admin/connectors/tracker?version=2", &cookie, (None, None)).await?;
        assert_eq!(status, 204);
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM connector_credentials").fetch_one(&f.pool).await?;
        let sealed: i64 = sqlx::query_scalar("SELECT count(*) FROM secrets WHERE ref LIKE 'secrets/connector-%'").fetch_one(&f.pool).await?;
        assert_eq!((rows, sealed), (0, 0));
        assert!(f.gateway.connector_credential(alice.id, "tracker").await.is_err(), "unknown now");
        Ok(())
    })
    .await
}

// TEST: GW-I-024
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_024_credentials_are_checked_and_only_for_connectors_that_send_one(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, SEED, async |f| {
        let (_, cookie) = f.admin().await?;
        assert_eq!(
            set(f, &cookie, "open", "synthetic").await?,
            409,
            "open takes no credential"
        );
        assert_eq!(set(f, &cookie, "absent", "synthetic").await?, 404);
        for value in ["", "has space", &"x".repeat(4097)] {
            assert_eq!(set(f, &cookie, "tracker", value).await?, 400, "{value:?}");
        }
        assert_eq!(set(f, &cookie, "tracker", &"x".repeat(4096)).await?, 204);
        let (status, _) = call(
            f,
            Method::PUT,
            "/api/v1/connectors/tracker/credential",
            &cookie,
            (Some(json!({"value": "x", "extra": 1})), None),
        )
        .await?;
        assert_eq!(status, 400);
        Ok(())
    })
    .await
}
