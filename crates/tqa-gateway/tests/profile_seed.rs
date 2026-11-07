//! GW-I-088 (ADR-0012, ADR-0028): `[[profiles]]` seeds shared profiles. An unchanged seed never overwrites an
//! administrator's edit, an edited seed is re-applied, a dropped entry is kept and marked removed, a deleted seeded
//! profile stays deleted until its seed changes, and a name the install does not have (or no longer has, after an API
//! delete) never stops start-up: the profile is shown unusable.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn seed(display: &str, route: &str) -> String {
    format!(
        "[[profiles]]\nname='team'\ndisplay_name='{display}'\nmodel_route='{route}'\ntools=['read_file']\n"
    )
}

/// The shared profile named `team`, as an administrator lists it.
async fn team(f: &Fixture, cookie: &str) -> Result<Option<Value>> {
    let (_, listed) = call(f, Method::GET, "/api/v1/profiles", cookie, (None, None)).await?;
    Ok(listed["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["name"] == "team")
        .cloned())
}

async fn restart_with(f: &mut Fixture, config: &str) -> TestResult {
    f.reconfigure(config)?;
    f.restart().await
}

async fn seed_state(f: &Fixture) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT seed_state FROM agent_profiles WHERE name='team' AND owner_id IS NULL",
    )
    .fetch_one(&f.pool)
    .await?)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_088_shared_profiles_follow_the_seed_rules(pool: PgPool) -> TestResult {
    support::run_restarting_with(pool, None, &seed("Team", "local"), async |f| {
        let (_, cookie) = f.admin().await?;
        let seeded = team(f, &cookie).await?.ok_or("seeded")?;
        assert_eq!(
            (
                &seeded["shared"],
                &seeded["version"],
                &seeded["display_name"]
            ),
            (&json!(true), &json!(1), &json!("Team"))
        );
        let path = format!("/api/v1/profiles/{}", seeded["id"].as_str().ok_or("id")?);
        // An administrator's edit survives a restart with an unchanged seed.
        let edited =
            json!({"name": "team", "display_name": "Edited", "model_route": "local", "version": 1});
        assert_eq!(
            call(f, Method::PUT, &path, &cookie, (Some(edited), None))
                .await?
                .0,
            200
        );
        restart_with(f, &seed("Team", "local")).await?;
        assert_eq!(
            team(f, &cookie).await?.ok_or("kept")?["display_name"],
            "Edited"
        );
        // An edited seed is re-applied.
        restart_with(f, &seed("Team 2", "local")).await?;
        let reapplied = team(f, &cookie).await?.ok_or("reapplied")?;
        assert_eq!(
            (&reapplied["display_name"], &reapplied["version"]),
            (&json!("Team 2"), &json!(3))
        );
        // Deleted, it stays deleted until its seed changes.
        let delete = format!("{path}?version=3");
        assert_eq!(
            call(f, Method::DELETE, &delete, &cookie, (None, None))
                .await?
                .0,
            204
        );
        restart_with(f, &seed("Team 2", "local")).await?;
        assert!(
            team(f, &cookie).await?.is_none(),
            "an unchanged seed does not bring it back"
        );
        restart_with(f, &seed("Team 3", "local")).await?;
        assert_eq!(
            team(f, &cookie).await?.ok_or("back")?["display_name"],
            "Team 3"
        );
        // Dropped from the file, it is kept and marked removed.
        restart_with(f, "").await?;
        assert!(team(f, &cookie).await?.is_some());
        assert_eq!(seed_state(f).await?.as_deref(), Some("removed"));
        // A name the install does not have never stops start-up: the entry is seeded and shown unusable.
        restart_with(f, &seed("Team 4", "no-such-route")).await?;
        let unusable = team(f, &cookie).await?.ok_or("seeded with a gone route")?;
        assert_eq!(
            (
                &unusable["display_name"],
                &unusable["usable"],
                &unusable["unusable_field"]
            ),
            (&json!("Team 4"), &json!(false), &json!("model_route"))
        );
        Ok(())
    })
    .await
}

const TRACKER: &str = "[[connectors]]\nname='tracker'\nurl='https://tracker.example/mcp'\nauth={scheme='bearer'}\n\
     [[profiles]]\nname='team'\ndisplay_name='Team'\nconnectors=['tracker']\n";

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_088_a_connector_deleted_through_the_api_never_stops_start_up(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, TRACKER, async |f| {
        let (_, cookie) = f.admin().await?;
        let seeded = team(f, &cookie).await?.ok_or("seeded")?;
        assert_eq!(
            (&seeded["usable"], &seeded.get("unusable_field")),
            (&json!(true), &None)
        );
        let delete = "/api/v1/admin/connectors/tracker?version=1";
        assert_eq!(
            call(f, Method::DELETE, delete, &cookie, (None, None))
                .await?
                .0,
            204
        );
        // The unchanged file names the deleted connector: the gateway starts, and the profile is kept as it was.
        f.restart().await?;
        let kept = team(f, &cookie).await?.ok_or("kept")?;
        assert_eq!(
            (
                &kept["version"],
                &kept["connectors"],
                &kept["usable"],
                &kept["unusable_field"]
            ),
            (
                &json!(1),
                &json!(["tracker"]),
                &json!(false),
                &json!("connectors")
            )
        );
        let path = format!("/api/v1/profiles/{}", kept["id"].as_str().ok_or("id")?);
        let (_, one) = call(f, Method::GET, &path, &cookie, (None, None)).await?;
        assert_eq!(one["unusable_field"], "connectors");
        Ok(())
    })
    .await
}
