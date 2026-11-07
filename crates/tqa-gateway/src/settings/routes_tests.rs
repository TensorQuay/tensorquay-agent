//! GW-I-226: a route's `stream_usage` became "on unless set false" (next release). Migration 0085 turns each stored
//! `false` (the old default, never applied to runs) into `true`, and the seeder's one-time digest upgrade keeps an
//! unchanged file's routes as they are: neither the stored record nor an administrator's edit is replaced because the
//! record's form changed. A real edit of the file still wins.
use super::seed;
use crate::settings::KeyedDigest;
use secrecy::SecretSlice;
use serde_json::Value;
use sqlx::{PgPool, Row};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn digest() -> KeyedDigest {
    let key: SecretSlice<u8> = vec![7; 32].into();
    KeyedDigest::new(&key, b"tqa seed digest v1")
}

fn file(model: &str) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='{model}'\n\
         auth={{kind='none'}}\n"
    ))?)
}

/// The route's record, version, provenance and seed digest.
async fn row(
    pool: &PgPool,
) -> Result<(Value, i64, String, Option<Vec<u8>>), Box<dyn std::error::Error>> {
    let row = sqlx::query(
        "SELECT document,version,last_set_by,seed_digest FROM model_routes WHERE name='local'",
    )
    .fetch_one(pool)
    .await?;
    Ok((
        row.try_get(0)?,
        row.try_get(1)?,
        row.try_get(2)?,
        row.try_get(3)?,
    ))
}

// TEST: GW-I-226
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_226_an_unchanged_file_keeps_its_routes_through_the_upgrade(
    pool: PgPool,
) -> TestResult {
    let digest = digest();
    let seeded = file("first")?;
    let mut connection = pool.acquire().await?;
    seed(&mut connection, &seeded, &digest).await?;
    let config = seeded.config(seeded.get("local").ok_or("no route")?.clone());
    let new_digest = digest.of(
        "routes/local",
        &serde_json::to_vec(&serde_json::to_value(&config)?)?,
    );
    let old_digest = digest.of(
        "routes/local",
        &serde_json::to_vec(&serde_json::to_value(config.legacy_form())?)?,
    );
    assert_ne!(new_digest, old_digest, "the form changed");
    // As the release before this one left it: the field written false, the digest over that form, and an
    // administrator's edit since.
    sqlx::query(
        "UPDATE model_routes SET document=jsonb_set(jsonb_set(document,'{stream_usage}','false'),'{model}','\"admin\"'), \
         seed_digest=$1, version=2, last_set_by='admin' WHERE name='local'",
    )
    .bind(&old_digest[..])
    .execute(&pool)
    .await?;
    // The upgrade's migration leaves a seeded route to the seeder.
    sqlx::raw_sql(include_str!("../../migrations/0085_route_stream_usage.sql"))
        .execute(&pool)
        .await?;
    let (document, version, ..) = row(&pool).await?;
    assert_eq!(
        (document["stream_usage"].clone(), version),
        (Value::Bool(false), 2),
        "seeded: untouched"
    );
    // The next start seeds the same file, which leaves the field out: the new default, on. The digest moves on, the
    // administrator's edit stays, and the change is a new version in the audit as the upgrade's.
    assert!(
        seed(&mut connection, &seeded, &digest).await?,
        "the old default turned on"
    );
    let (document, version, by, stored) = row(&pool).await?;
    assert_eq!(document["stream_usage"], Value::Bool(true));
    assert_eq!(
        (document["model"].as_str(), version, by.as_str()),
        (Some("admin"), 3, "admin"),
        "the edit kept"
    );
    assert_eq!(stored, Some(new_digest.to_vec()));
    let audit: (String, String, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT actor_kind,action,before_version,after_version FROM admin_audit \
         WHERE area='routes' AND item='local' ORDER BY at DESC, id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        audit,
        ("seed".into(), "replace".into(), Some(2), Some(3)),
        "recorded as the upgrade's"
    );
    assert!(
        !seed(&mut connection, &seeded, &digest).await?,
        "and the upgrade happens once"
    );
    // An edited file still wins.
    assert!(seed(&mut connection, &file("second")?, &digest).await?);
    let (document, version, by, _) = row(&pool).await?;
    assert_eq!(
        (document["model"].as_str(), version, by.as_str()),
        (Some("second"), 4, "seed")
    );
    assert_eq!(
        document.get("stream_usage"),
        None,
        "the file left it out: on"
    );
    Ok(())
}

// TEST: GW-I-226
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_226_an_explicit_false_in_the_file_is_honoured(pool: PgPool) -> TestResult {
    let digest = digest();
    let explicit = tqa_llm::Registry::from_toml(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='m'\n\
         auth={kind='none'}\nstream_usage=false\n",
    )?;
    let mut connection = pool.acquire().await?;
    seed(&mut connection, &explicit, &digest).await?;
    sqlx::raw_sql(include_str!("../../migrations/0085_route_stream_usage.sql"))
        .execute(&pool)
        .await?;
    assert!(
        !seed(&mut connection, &explicit, &digest).await?,
        "the file unchanged: nothing written"
    );
    let (document, version, ..) = row(&pool).await?;
    assert_eq!(
        (document["stream_usage"].clone(), version),
        (Value::Bool(false), 1),
        "off, as the file says"
    );
    Ok(())
}

// TEST: GW-I-226
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_226_the_migration_turns_on_an_administrators_route_left_at_the_old_default(
    pool: PgPool,
) -> TestResult {
    let mut connection = pool.acquire().await?;
    seed(&mut connection, &file("first")?, &digest()).await?;
    // A route added in the web app before the upgrade: no seed digest, the form's old default false.
    sqlx::query(
        "UPDATE model_routes SET document=jsonb_set(document,'{stream_usage}','false'), seed_digest=NULL, \
         seed_state=NULL, last_set_by='admin' WHERE name='local'",
    )
    .execute(&pool)
    .await?;
    let version = row(&pool).await?.1;
    sqlx::raw_sql(include_str!("../../migrations/0085_route_stream_usage.sql"))
        .execute(&pool)
        .await?;
    let (document, after, ..) = row(&pool).await?;
    assert_eq!(
        (document["stream_usage"].clone(), after),
        (Value::Bool(true), version + 1)
    );
    Ok(())
}

// TEST: GW-I-226
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_226_the_migration_leaves_a_true_or_an_absent_setting_alone(
    pool: PgPool,
) -> TestResult {
    let mut connection = pool.acquire().await?;
    seed(&mut connection, &file("first")?, &digest()).await?;
    let (before, version, ..) = row(&pool).await?;
    assert_eq!(before.get("stream_usage"), None);
    sqlx::raw_sql(include_str!("../../migrations/0085_route_stream_usage.sql"))
        .execute(&pool)
        .await?;
    assert_eq!(row(&pool).await?.1, version, "no false, no change");
    Ok(())
}
