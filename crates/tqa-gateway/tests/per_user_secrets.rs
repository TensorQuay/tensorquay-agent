//! A user's own credentials stay theirs (ADR-0033, FR-19, FR-24).
//! - SEC-031: an administrator can neither overwrite nor delete a user's connector credential, nor make a route or a
//!   connector use a user's credential for everyone.
//! - GW-I-092: every change a user makes to their own credentials is audited, never with a value.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

const SEED: &str = "[[connectors]]\nname='tracker'\nurl='https://mcp.tracker.example/mcp'\n\
    auth={scheme='bearer'}\nsecret_ref='secrets/tracker-shared'\n";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// The store name of `user`'s own credential for the connector `connector`.
fn connector_name(user: uuid::Uuid, connector: &str) -> String {
    format!(
        "connector-{}",
        hex(&Sha256::digest(format!("{user}\n{connector}").as_bytes()))
    )
}

fn detail(body: &Value) -> (Value, Value) {
    let detail = &body["error"]["details"][0];
    (detail["field"].clone(), detail["reason"].clone())
}

async fn own_credential(f: &Fixture, cookie: &str, value: &str) -> Result<()> {
    let (status, body) = call(
        f,
        Method::PUT,
        "/api/v1/connectors/tracker/credential",
        cookie,
        (Some(json!({"value": value})), None),
    )
    .await?;
    assert_eq!(status, 204, "{body}");
    Ok(())
}

// TEST: SEC-031
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_031_an_administrator_cannot_overwrite_list_or_delete_a_users_connector_credential(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, SEED, async |f| {
        let (_, admin) = f.admin().await?;
        let (bob, bob_cookie) = f.member("bob@example.invalid").await?;
        own_credential(f, &bob_cookie, "synthetic-bob").await?;
        let name = connector_name(bob.id, "tracker");
        for name in [name.clone(), name.to_uppercase()] {
            let (status, body) = call(
                f,
                Method::PUT,
                &format!("/api/v1/admin/secrets/{name}"),
                &admin,
                (Some(json!({"value": "synthetic-admin"})), None),
            )
            .await?;
            assert_eq!(
                (status, detail(&body)),
                (400, (json!("name"), json!("reserved_prefix"))),
                "{name}"
            );
            let (status, body) = call(
                f,
                Method::DELETE,
                &format!("/api/v1/admin/secrets/{name}?version=1"),
                &admin,
                (None, None),
            )
            .await?;
            assert_eq!(
                (status, detail(&body)),
                (400, (json!("name"), json!("reserved_prefix"))),
                "{name}"
            );
        }
        let (status, listed) = call(
            f,
            Method::GET,
            "/api/v1/admin/secrets",
            &admin,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        assert!(
            !listed.to_string().contains(&name),
            "a user's own credential is not listed to administrators: {listed}"
        );
        // Bob's calls still use Bob's value.
        let credential = f
            .gateway
            .connector_credential(bob.id, "tracker")
            .await?
            .ok_or("bob's credential")?;
        assert_eq!(credential.value.expose_secret(), "synthetic-bob");
        Ok(())
    })
    .await
}

// TEST: SEC-031
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_031_a_route_or_connector_cannot_name_a_users_credential(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, SEED, async |f| {
        let (_, admin) = f.admin().await?;
        let (bob, _) = f.member("bob@example.invalid").await?;
        let personal = [
            format!("secrets/{}", connector_name(bob.id, "tracker")),
            format!("secrets/git-{}", "a".repeat(64)),
            "secrets/Git-abc".to_owned(),
        ];
        for reference in &personal {
            let (status, body) = call(
                f,
                Method::PUT,
                "/api/v1/admin/connectors/shared",
                &admin,
                (
                    Some(json!({"url": "https://mcp.shared.example/mcp", "auth": {"scheme": "bearer"},
                                "secret_ref": reference})),
                    None,
                ),
            )
            .await?;
            assert_eq!(
                (status, detail(&body)),
                (400, (json!("secret_ref"), json!("secret_ref_reserved"))),
                "{reference}"
            );
            let (status, body) = call(
                f,
                Method::PUT,
                "/api/v1/admin/routes/keyed",
                &admin,
                (
                    Some(json!({"protocol": "openai_chat", "base_url": "https://models.example/v1",
                                "model": "m", "auth": {"kind": "bearer"}, "key_ref": reference})),
                    None,
                ),
            )
            .await?;
            assert_eq!(
                (status, detail(&body)),
                (400, (json!("key_ref"), json!("key_ref_reserved"))),
                "{reference}"
            );
        }
        Ok(())
    })
    .await
}

// TEST: SEC-031
#[test]
fn sec_031_a_file_that_names_a_users_credential_stops_the_gateway() -> TestResult {
    let file = "[[routes]]\nname='keyed'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\n\
                model='m'\nauth={kind='bearer'}\nkey_ref='secrets/connector-abc'\n";
    let error = tqa_llm::Registry::from_toml(file)
        .err()
        .ok_or("the models file accepts it")?;
    assert_eq!(
        error.reason().map(tqa_llm::RouteInvalid::code),
        Some("key_ref_reserved")
    );
    let image = format!("sha256:{}", "a".repeat(64));
    let config = format!(
        "listen='127.0.0.1:8080'\npublic_origin='http://localhost:8080'\nmodels_path='/etc/tqa/models.toml'\n\
         [storage]\nroot='/srv/tqa'\n[runtime]\nbackend='docker'\nimage='{image}'\n\
         [[connectors]]\nname='shared'\nurl='https://mcp.shared.example/mcp'\nauth={{scheme='bearer'}}\n\
         secret_ref='secrets/git-abc'\n"
    );
    assert!(
        tqa_gateway::GatewayConfig::from_toml(&config).is_err(),
        "a connector in the file may not name a user's credential"
    );
    Ok(())
}

async fn audited(f: &Fixture, area: &str) -> Result<Vec<(String, uuid::Uuid, String, String)>> {
    Ok(sqlx::query_as(
        "SELECT actor_kind, actor_user_id, item, action FROM admin_audit WHERE area=$1 ORDER BY at, id",
    )
    .bind(area)
    .fetch_all(&f.pool)
    .await?)
}

// TEST: GW-I-092
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_092_a_users_changes_to_their_own_credentials_are_audited_without_values(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, SEED, async |f| {
        // The install's first account is its administrator.
        f.admin().await?;
        let (bob, cookie) = f.member("bob@example.invalid").await?;
        own_credential(f, &cookie, "synthetic-one").await?;
        own_credential(f, &cookie, "synthetic-two").await?;
        let (status, _) = call(
            f,
            Method::DELETE,
            "/api/v1/connectors/tracker/credential",
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 204);
        let item = format!("secrets/{}", connector_name(bob.id, "tracker"));
        let user = || ("user".to_owned(), bob.id, item.clone());
        let expected: Vec<_> = ["create", "replace", "delete"]
            .into_iter()
            .map(|action| {
                let (kind, id, item) = user();
                (kind, id, item, action.to_owned())
            })
            .collect();
        assert_eq!(audited(f, "connector_credentials").await?, expected);
        // Never a value, anywhere in the audit.
        let all: Vec<String> = sqlx::query_scalar("SELECT row_to_json(a)::text FROM admin_audit a")
            .fetch_all(&f.pool)
            .await?;
        for value in ["synthetic-one", "synthetic-two"] {
            assert!(all.iter().all(|row| !row.contains(value)), "{value}");
        }
        Ok(())
    })
    .await
}
