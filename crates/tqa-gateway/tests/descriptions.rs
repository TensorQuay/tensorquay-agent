//! GW-I-132: an administrator's one-line description of a model route and of an environment, shown to people where
//! they pick one (display only). Routes take it through the admin API and show it on `GET /api/v1/models`;
//! environments take it through the admin API and `[[templates]]` in `gateway.toml`, and show it on
//! `GET /api/v1/templates`. A bad value is refused with `description_invalid`; `null` clears it.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const PIN: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const OTHER: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

/// Values the API refuses: empty, padded, two lines, a control character, past 160 characters, not a string.
fn refused_values() -> Vec<Value> {
    vec![
        json!(""),
        json!(" padded"),
        json!("two\nlines"),
        json!("tab\there"),
        json!("x".repeat(161)),
        json!(5),
    ]
}

fn reason(body: &Value) -> (Value, Value) {
    let detail = &body["error"]["details"][0];
    (detail["field"].clone(), detail["reason"].clone())
}

/// The item called `name` in a list response's `items`.
async fn listed(f: &Fixture, cookie: &str, path: &str, name: &str) -> Result<Value> {
    let (status, body) = call(f, Method::GET, path, cookie, (None, None)).await?;
    assert_eq!(status, 200, "{body}");
    Ok(body["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["name"] == name)
        .cloned()
        .ok_or("listed")?)
}

// TEST: GW-I-132
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_132_a_route_describes_itself_to_the_people_who_pick_it(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let path = "/api/v1/admin/routes/quick";
        let route = json!({"protocol": "openai_chat", "base_url": "https://models.example/v1", "model": "quick-1",
            "auth": {"kind": "none"}, "description": "For quick lookups and short answers"});
        let (status, created) = call(f, Method::PUT, path, &cookie, (Some(route.clone()), Some("create-quick"))).await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(created["description"], "For quick lookups and short answers");
        let admin = listed(f, &cookie, "/api/v1/admin/routes", "quick").await?;
        assert_eq!(admin["description"], "For quick lookups and short answers");
        let picked = listed(f, &cookie, "/api/v1/models", "quick").await?;
        assert_eq!(picked["description"], "For quick lookups and short answers");

        // Refused values change nothing.
        for bad in refused_values() {
            let mut body = route.clone();
            body["description"] = bad.clone();
            body["version"] = json!(1);
            let (status, refused) = call(f, Method::PUT, path, &cookie, (Some(body), None)).await?;
            assert_eq!((status, reason(&refused)), (400, (json!("description"), json!("description_invalid"))), "{bad}");
        }
        // 160 characters is the most; null clears it, and a route without one shows null.
        let mut longest = route.clone();
        longest["description"] = json!("y".repeat(160));
        longest["version"] = json!(1);
        assert_eq!(call(f, Method::PUT, path, &cookie, (Some(longest), None)).await?.0, 200);
        let mut cleared = route.clone();
        cleared["description"] = Value::Null;
        cleared["version"] = json!(2);
        let (status, body) = call(f, Method::PUT, path, &cookie, (Some(cleared), None)).await?;
        assert_eq!((status, &body["description"]), (200, &Value::Null), "{body}");
        assert_eq!(listed(f, &cookie, "/api/v1/models", "quick").await?["description"], Value::Null);
        Ok(())
    })
    .await
}

// TEST: GW-I-132
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_132_an_environment_describes_itself_from_the_file_or_the_api(
    pool: PgPool,
) -> TestResult {
    let config = format!(
        "[[templates]]\nname='workstation'\ndisplay_name='Workstation'\ndescription='Everyday work: documents, data and code'\nimage='{PIN}'\ncapabilities=['shell']\ndefault=true\n"
    );
    support::run_configured(pool, None, &config, async |f| {
        let (_, cookie) = f.admin().await?;
        // From the file, on the list people pick from and on the administrators' list.
        let picked = listed(f, &cookie, "/api/v1/templates", "workstation").await?;
        assert_eq!(
            picked["description"],
            "Everyday work: documents, data and code"
        );
        let admin = listed(f, &cookie, "/api/v1/admin/templates", "workstation").await?;
        assert_eq!(
            admin["description"],
            "Everyday work: documents, data and code"
        );

        // Through the API: set, refused, cleared.
        let path = "/api/v1/admin/templates/analysis";
        let body = json!({"display_name": "Analysis", "image": OTHER, "capabilities": ["shell"],
            "description": "Spreadsheets, statistics and charts"});
        let (status, created) = call(
            f,
            Method::PUT,
            path,
            &cookie,
            (Some(body.clone()), Some("create-analysis")),
        )
        .await?;
        assert_eq!(
            (status, &created["description"]),
            (201, &json!("Spreadsheets, statistics and charts")),
            "{created}"
        );
        for bad in refused_values() {
            let mut refused = body.clone();
            refused["description"] = bad.clone();
            refused["version"] = json!(1);
            let (status, answer) =
                call(f, Method::PUT, path, &cookie, (Some(refused), None)).await?;
            assert_eq!(
                (status, reason(&answer)),
                (400, (json!("description"), json!("description_invalid"))),
                "{bad}"
            );
        }
        let mut cleared = body.clone();
        cleared["description"] = Value::Null;
        cleared["version"] = json!(1);
        let (status, answer) = call(f, Method::PUT, path, &cookie, (Some(cleared), None)).await?;
        assert_eq!(
            (status, &answer["description"]),
            (200, &Value::Null),
            "{answer}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-132
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_132_a_seed_re_apply_keeps_a_description_set_through_the_api(
    pool: PgPool,
) -> TestResult {
    let config = format!(
        "[[templates]]\nname='workstation'\ndisplay_name='Workstation'\nimage='{PIN}'\ncapabilities=['shell']\ndefault=true\n\
         [[templates]]\nname='notes'\ndisplay_name='Notes'\nimage='{OTHER}'\ncapabilities=['shell']\n"
    );
    support::run_restarting_with(pool, None, &config, async |f| {
        let (_, cookie) = f.admin().await?;
        let path = "/api/v1/admin/templates/notes";
        let body = json!({"display_name": "Notes", "image": OTHER, "capabilities": ["shell"],
            "description": "Set by an administrator", "version": 1});
        let (status, written) = call(f, Method::PUT, path, &cookie, (Some(body), None)).await?;
        assert_eq!(status, 200, "{written}");
        // The file changes another field of the seeded template, and names no description: the seed re-applies, and
        // the administrator's description stays.
        f.reconfigure(&config.replace("display_name='Notes'", "display_name='Notes 2'"))?;
        f.restart().await?;
        let admin = listed(f, &cookie, "/api/v1/admin/templates", "notes").await?;
        assert_eq!(
            (&admin["display_name"], &admin["description"]),
            (&json!("Notes 2"), &json!("Set by an administrator"))
        );
        Ok(())
    })
    .await
}
