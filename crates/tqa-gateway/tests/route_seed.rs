//! GW-I-059 (ADR-0028): model routes live in the database, seeded from models.toml; a seed route applies only when it
//! changed since it was last imported, so an administrator's change survives a restart and a file edit still wins.
//! GW-I-216 (ADR-0028's N2): a file route's `display_name` goes to the route's display name, never into its stored
//! record; a route without one keeps its digest, a file without one keeps an administrator's, and an unclean one stops
//! the start.

#[path = "support/fixture.rs"]
mod support;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::fmt::Write as _;
use support::{Fixture, TestResult};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn registry(routes: &[(&str, &str)]) -> Result<tqa_llm::Registry> {
    let mut source = String::new();
    for (name, model) in routes {
        writeln!(
            source,
            "[[routes]]\nname='{name}'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='{model}'\nauth={{kind='none'}}"
        )?;
    }
    Ok(tqa_llm::Registry::from_toml(&source)?)
}

/// Each route the running gateway lists, with its model.
async fn models(f: &Fixture, cookie: &str) -> Result<Vec<(String, String)>> {
    let body: Value = f.get("/api/v1/models", cookie).await?.json().await?;
    let items = body["items"].as_array().ok_or("items")?;
    Ok(items
        .iter()
        .map(|item| {
            (
                item["name"].as_str().unwrap_or_default().to_owned(),
                item["model"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect())
}

/// Restarts the gateway with `seed` as its models file.
async fn restart(f: &mut Fixture, seed: &[(&str, &str)]) -> TestResult {
    f.reseed(registry(seed)?);
    f.restart().await
}

/// The `version`, `last_set_by` and `seed_state` of one route.
async fn provenance(pool: &PgPool, name: &str) -> Result<(i64, String, Option<String>)> {
    let row = sqlx::query("SELECT version,last_set_by,seed_state FROM model_routes WHERE name=$1")
        .bind(name)
        .fetch_one(pool)
        .await?;
    Ok((row.try_get(0)?, row.try_get(1)?, row.try_get(2)?))
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_059_an_unchanged_seed_keeps_an_admin_change_and_an_edited_seed_wins(
    pool: PgPool,
) -> TestResult {
    let local = |model: &str| vec![("local".to_owned(), model.to_owned())];
    support::run_restarting_with(pool.clone(), Some(registry(&[("local", "first")])?), "", async |f| {
        let (_, cookie) = f.admin().await?;
        assert_eq!(models(f, &cookie).await?, local("first"));
        assert_eq!(provenance(&f.pool, "local").await?, (1, "seed".into(), Some("current".into())));
        // An administrator's change, as the routes API will write it.
        sqlx::query("UPDATE model_routes SET document=jsonb_set(document,'{model}','\"admin\"'), version=version+1, last_set_by='admin' WHERE name='local'")
            .execute(&f.pool)
            .await?;
        restart(f, &[("local", "first")]).await?;
        assert_eq!(models(f, &cookie).await?, local("admin"), "the same seed keeps it");
        assert_eq!(provenance(&f.pool, "local").await?, (2, "admin".into(), Some("current".into())));
        restart(f, &[("local", "second")]).await?;
        assert_eq!(models(f, &cookie).await?, local("second"), "an edited seed wins");
        assert_eq!(provenance(&f.pool, "local").await?, (3, "seed".into(), Some("current".into())));
        let audit: Vec<(String, String, Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT actor_kind,action,before_version,after_version FROM admin_audit WHERE area='routes' AND item='local' ORDER BY at, id",
        )
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(audit, vec![("seed".into(), "create".into(), None, Some(1)), ("seed".into(), "replace".into(), Some(2), Some(3))]);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_059_a_route_dropped_from_the_seed_is_kept_and_marked(pool: PgPool) -> TestResult {
    support::run_restarting_with(
        pool,
        Some(registry(&[("local", "first"), ("extra", "kept")])?),
        "",
        async |f| {
            let (_, cookie) = f.admin().await?;
            let (digest, document): (Vec<u8>, Value) =
                sqlx::query_as("SELECT seed_digest,document FROM model_routes WHERE name='extra'")
                    .fetch_one(&f.pool)
                    .await?;
            assert_ne!(
                digest,
                Sha256::digest(serde_json::to_vec(&document)?).to_vec(),
                "the digest is keyed"
            );
            restart(f, &[("local", "first")]).await?;
            assert_eq!(
                models(f, &cookie).await?,
                vec![
                    ("extra".into(), "kept".into()),
                    ("local".into(), "first".into())
                ],
                "never deleted silently"
            );
            assert_eq!(
                provenance(&f.pool, "extra").await?.2.as_deref(),
                Some("removed")
            );
            assert_eq!(
                provenance(&f.pool, "local").await?.2.as_deref(),
                Some("current")
            );
            restart(f, &[("local", "first"), ("extra", "kept")]).await?;
            assert_eq!(
                provenance(&f.pool, "extra").await?,
                (1, "seed".into(), Some("current".into())),
                "listed again unchanged: current again, with no new version"
            );
            Ok(())
        },
    )
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_059_a_stored_route_the_validator_refuses_stops_the_start(pool: PgPool) -> TestResult {
    support::run_restarting_with(pool, Some(registry(&[("local", "first")])?), "", async |f| {
        // A rule the validator enforces (an empty model), then a record that is not a route at all.
        for document in [r#"{"name":"local","protocol":"openai_chat","base_url":"https://models.example/v1","model":""}"#, r#"{"unknown":1}"#] {
            sqlx::query("UPDATE model_routes SET document=$1::jsonb WHERE name='local'")
                .bind(document)
                .execute(&f.pool)
                .await?;
            let refused = f.start_again().await;
            assert!(
                matches!(refused, Err(tqa_gateway::GatewayError::Configuration(_))),
                "{document}: {refused:?}"
            );
        }
        Ok(())
    })
    .await
}

/// Routes with a model and, when given, a display name.
fn named(routes: &[(&str, &str, Option<&str>)]) -> Result<tqa_llm::Registry> {
    let mut source = String::new();
    for (name, model, display) in routes {
        writeln!(
            source,
            "[[routes]]\nname='{name}'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='{model}'\nauth={{kind='none'}}"
        )?;
        if let Some(display) = display {
            writeln!(source, "display_name={}", serde_json::to_string(display)?)?;
        }
    }
    Ok(tqa_llm::Registry::from_toml(&source)?)
}

/// A route's stored display name, and whether its stored record holds one.
async fn display(pool: &PgPool, name: &str) -> Result<(Option<String>, bool)> {
    let row = sqlx::query(
        "SELECT display_name, document ? 'display_name' FROM model_routes WHERE name=$1",
    )
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok((row.try_get(0)?, row.try_get(1)?))
}

// TEST: GW-I-216
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_216_a_file_routes_display_name_is_stored_beside_its_record(
    pool: PgPool,
) -> TestResult {
    let seed = named(&[("local", "m", Some("Local model")), ("spare", "m", None)])?;
    support::run_restarting_with(pool, Some(seed), "", async |f| {
        let (_, cookie) = f.admin().await?;
        assert_eq!(
            display(&f.pool, "local").await?,
            (Some("Local model".into()), false)
        );
        assert_eq!(display(&f.pool, "spare").await?, (None, false));
        let body: Value = f.get("/api/v1/models", &cookie).await?.json().await?;
        let local = body["items"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["name"] == "local"))
            .ok_or("no local route")?;
        assert_eq!(local["display_name"], "Local model", "{body}");
        // A changed name is a seed change; the route without one keeps its digest and is not seeded again.
        f.reseed(named(&[
            ("local", "m", Some("Local")),
            ("spare", "m", None),
        ])?);
        f.restart().await?;
        assert_eq!(
            display(&f.pool, "local").await?,
            (Some("Local".into()), false)
        );
        assert_eq!(
            provenance(&f.pool, "local").await?,
            (2, "seed".into(), Some("current".into()))
        );
        assert_eq!(
            provenance(&f.pool, "spare").await?,
            (1, "seed".into(), Some("current".into()))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-216
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_216_a_file_without_a_name_keeps_the_administrators_and_an_unclean_one_stops_the_start(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(
        pool,
        Some(named(&[("local", "first", None)])?),
        "",
        async |f| {
            f.admin().await?;
            sqlx::query("UPDATE model_routes SET display_name='Admin name' WHERE name='local'")
                .execute(&f.pool)
                .await?;
            // The file changes the model, not the name: the record is seeded again, the administrator's name stays.
            f.reseed(named(&[("local", "second", None)])?);
            f.restart().await?;
            assert_eq!(provenance(&f.pool, "local").await?.0, 2);
            assert_eq!(
                display(&f.pool, "local").await?,
                (Some("Admin name".into()), false)
            );
            for unclean in [" padded", "", "two\nlines", "x".repeat(81).as_str()] {
                f.reseed(named(&[("local", "second", Some(unclean))])?);
                let refused = f.start_again().await;
                assert!(
                    matches!(refused, Err(tqa_gateway::GatewayError::Configuration(_))),
                    "{unclean:?}: {refused:?}"
                );
            }
            Ok(())
        },
    )
    .await
}
