//! GW-I-065 (ADR-0028): a seeded item an administrator deletes stays deleted across restarts, in every settings area;
//! an edited seed entry (a new value) brings it back, and an item re-created through the API is not overwritten by an
//! unchanged seed.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The item named `key` = `name` in an admin list, if listed.
async fn listed(
    f: &Fixture,
    cookie: &str,
    list: &str,
    key: &str,
    name: &str,
) -> Result<Option<Value>> {
    let (status, body) = call(f, Method::GET, list, cookie, (None, None)).await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item[key] == name)
        .cloned())
}

fn registry(model: &str) -> Result<tqa_llm::Registry> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='fixture'\n\
         [[routes]]\nname='seeded'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='{model}'\n"
    ))?)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_065_a_deleted_seeded_route_stays_deleted_until_its_seed_changes(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, Some(registry("first")?), "", async |f| {
        let (_, cookie) = f.admin().await?;
        let (list, path) = ("/api/v1/admin/routes", "/api/v1/admin/routes/seeded");
        assert_eq!(call(f, Method::DELETE, &format!("{path}?version=1"), &cookie, (None, None)).await?.0, 204);
        f.restart().await?;
        assert_eq!(listed(f, &cookie, list, "name", "seeded").await?, None, "an unchanged seed does not bring it back");
        let body = json!({"protocol": "openai_chat", "base_url": "https://models.example/v1", "model": "admin"});
        assert_eq!(call(f, Method::PUT, path, &cookie, (Some(body), None)).await?.0, 201);
        f.restart().await?;
        let again = listed(f, &cookie, list, "name", "seeded").await?.ok_or("re-created")?;
        assert_eq!((&again["model"], &again["last_set_by"]), (&json!("admin"), &json!("admin")), "re-created, kept");
        assert_eq!(call(f, Method::DELETE, &format!("{path}?version=1"), &cookie, (None, None)).await?.0, 204);
        f.reseed(registry("second")?);
        f.restart().await?;
        let back = listed(f, &cookie, list, "name", "seeded").await?.ok_or("an edited seed brings it back")?;
        assert_eq!((&back["model"], &back["last_set_by"]), (&json!("second"), &json!("seed")));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_065_a_deleted_seeded_egress_rule_stays_deleted_until_its_seed_changes(
    pool: PgPool,
) -> TestResult {
    let seeded =
        |ports: &str| format!("[egress]\n[[egress.allow]]\nhost='seeded.example'\nports=[{ports}]");
    support::run_restarting_with(pool, None, &seeded("443"), async |f| {
        let (_, cookie) = f.admin().await?;
        let (list, path) = (
            "/api/v1/admin/egress",
            "/api/v1/admin/egress/seeded.example",
        );
        assert_eq!(
            call(
                f,
                Method::DELETE,
                &format!("{path}?version=1"),
                &cookie,
                (None, None)
            )
            .await?
            .0,
            204
        );
        f.restart().await?;
        assert_eq!(
            listed(f, &cookie, list, "host", "seeded.example").await?,
            None,
            "an unchanged seed does not bring it back"
        );
        assert_eq!(
            call(
                f,
                Method::PUT,
                path,
                &cookie,
                (Some(json!({"ports": [8443]})), None)
            )
            .await?
            .0,
            201
        );
        f.restart().await?;
        let again = listed(f, &cookie, list, "host", "seeded.example")
            .await?
            .ok_or("re-created")?;
        assert_eq!(
            (&again["ports"], &again["last_set_by"]),
            (&json!([8443]), &json!("admin")),
            "re-created, kept"
        );
        assert_eq!(
            call(
                f,
                Method::DELETE,
                &format!("{path}?version=1"),
                &cookie,
                (None, None)
            )
            .await?
            .0,
            204
        );
        f.reconfigure(&seeded("443, 80"))?;
        f.restart().await?;
        let back = listed(f, &cookie, list, "host", "seeded.example")
            .await?
            .ok_or("an edited seed brings it back")?;
        assert_eq!(
            (&back["ports"], &back["last_set_by"]),
            (&json!([80, 443]), &json!("seed"))
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_065_a_deleted_seeded_secret_stays_deleted_until_its_seed_changes(
    pool: PgPool,
) -> TestResult {
    let seeded = |value: &str| format!("[secrets]\n\"secrets/seeded\"='{value}'\n");
    support::run_restarting(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        f.seed_secrets(&seeded("first"))?;
        f.restart().await?;
        let (list, path) = ("/api/v1/admin/secrets", "/api/v1/admin/secrets/seeded");
        let first = listed(f, &cookie, list, "ref", "secrets/seeded")
            .await?
            .ok_or("seeded")?;
        assert_eq!(first["seed"], json!("current"));
        assert_eq!(
            call(
                f,
                Method::DELETE,
                &format!("{path}?version=1"),
                &cookie,
                (None, None)
            )
            .await?
            .0,
            204
        );
        f.restart().await?;
        assert_eq!(
            listed(f, &cookie, list, "ref", "secrets/seeded").await?,
            None,
            "an unchanged seed does not bring it back"
        );
        assert_eq!(
            call(
                f,
                Method::PUT,
                path,
                &cookie,
                (Some(json!({"value": "admin"})), None)
            )
            .await?
            .0,
            201
        );
        f.restart().await?;
        let stored = f
            .gateway
            .secret_store()
            .get(&"secrets/seeded".parse()?)
            .await?
            .ok_or("re-created")?;
        assert_eq!(
            secrecy::ExposeSecret::expose_secret(&stored),
            "admin",
            "re-created, kept"
        );
        assert_eq!(
            call(
                f,
                Method::DELETE,
                &format!("{path}?version=1"),
                &cookie,
                (None, None)
            )
            .await?
            .0,
            204
        );
        f.seed_secrets(&seeded("second"))?;
        f.restart().await?;
        let stored = f
            .gateway
            .secret_store()
            .get(&"secrets/seeded".parse()?)
            .await?
            .ok_or("an edited seed brings it back")?;
        assert_eq!(secrecy::ExposeSecret::expose_secret(&stored), "second");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_065_an_item_the_seed_stops_listing_is_new_when_it_is_listed_again(
    pool: PgPool,
) -> TestResult {
    let listing = "[egress]\n[[egress.allow]]\nhost='seeded.example'\nports=[443]";
    support::run_restarting_with(pool, None, listing, async |f| {
        let (_, cookie) = f.admin().await?;
        let (list, path) = (
            "/api/v1/admin/egress",
            "/api/v1/admin/egress/seeded.example",
        );
        assert_eq!(
            call(
                f,
                Method::DELETE,
                &format!("{path}?version=1"),
                &cookie,
                (None, None)
            )
            .await?
            .0,
            204
        );
        f.reconfigure("")?;
        f.restart().await?;
        let tombstones: i64 = sqlx::query_scalar("SELECT count(*) FROM seed_tombstones")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(
            tombstones, 0,
            "the file no longer lists it, so the tombstone is forgotten"
        );
        f.reconfigure(listing)?;
        f.restart().await?;
        let back = listed(f, &cookie, list, "host", "seeded.example")
            .await?
            .ok_or("listed again, imported again")?;
        assert_eq!(
            (&back["seed"], &back["last_set_by"]),
            (&json!("current"), &json!("seed"))
        );
        Ok(())
    })
    .await
}
