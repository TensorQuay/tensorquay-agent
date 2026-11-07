//! GW-I-156 (F20): internet-access presets, built-in named rule sets. `GET /api/v1/admin/egress/presets` lists them with
//! how much of each is applied; `POST …/{id}` adds a preset's missing rules tagged with it, keeping a host that already
//! has a rule; `DELETE …/{id}` removes the rules still tagged. An administrator's edit untags a rule. `open_web` is the
//! only way to store the any-host rule '*'. Members see only whether their tasks reach anything (`/auth/me`).

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{TestResult, call};

const PRESETS: &str = "/api/v1/admin/egress/presets";

fn hosts(list: &Value, key: &str) -> Vec<String> {
    let mut hosts: Vec<String> = list[key]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["host"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    hosts.sort();
    hosts
}

async fn applied(
    f: &support::Fixture,
    cookie: &str,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let (status, list) = call(f, Method::GET, PRESETS, cookie, (None, None)).await?;
    assert_eq!(status, 200, "{list}");
    Ok(list["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .map(|item| {
            (
                item["id"].as_str().unwrap_or_default().to_owned(),
                item["applied"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect())
}

async fn internet(f: &support::Fixture, cookie: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let (status, me) = call(f, Method::GET, "/api/v1/auth/me", cookie, (None, None)).await?;
    assert_eq!(status, 200);
    Ok(me["internet"].clone())
}

// TEST: GW-I-156
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_156_presets_add_their_rules_tagged_and_remove_only_those(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let (_, member) = f.member("member@example.invalid").await?;
        assert_eq!(internet(f, &member).await?, json!("none"), "a fresh install reaches nothing");
        assert_eq!(
            applied(f, &cookie).await?,
            [("package_registries".into(), "none".into()), ("open_web".into(), "none".into()), ("code_hosting".into(), "none".into())]
        );
        // A host the administrator already allowed is kept as it was.
        let body = Some(json!({"ports": [8443]}));
        assert_eq!(call(f, Method::PUT, "/api/v1/admin/egress/pypi.org", &cookie, (body, None)).await?.0, 201);
        let path = format!("{PRESETS}/package_registries");
        let (status, answer) = call(f, Method::POST, &path, &cookie, (None, Some("apply-1"))).await?;
        assert_eq!(status, 200, "{answer}");
        assert_eq!(hosts(&answer, "kept"), ["pypi.org"]);
        assert_eq!(
            hosts(&answer, "added"),
            ["crates.io", "files.pythonhosted.org", "index.crates.io", "proxy.golang.org", "registry.npmjs.org", "static.crates.io", "sum.golang.org"]
        );
        assert!(answer["added"].as_array().ok_or("added")?.iter().all(|rule| rule["ports"] == json!([443])));
        let (status, replayed) = call(f, Method::POST, &path, &cookie, (None, Some("apply-1"))).await?;
        assert_eq!((status, &replayed), (200, &answer), "a retry replays");
        let (_, again) = call(f, Method::POST, &path, &cookie, (None, Some("apply-2"))).await?;
        assert_eq!(hosts(&again, "added"), Vec::<String>::new(), "a second apply adds nothing");
        // pypi.org is the administrator's own (8443), so the preset isn't all in place: partial.
        assert_eq!(applied(f, &cookie).await?[0], ("package_registries".into(), "partial".into()));
        assert_eq!(internet(f, &member).await?, json!("some"));
        // The list shows each rule's preset; an edit makes a rule the administrator's own.
        let (_, list) = call(f, Method::GET, "/api/v1/admin/egress", &cookie, (None, None)).await?;
        let preset = |host: &str| list["items"].as_array().and_then(|items| items.iter().find(|item| item["host"] == host)).map(|item| item["preset"].clone());
        assert_eq!(preset("pypi.org"), Some(Value::Null));
        assert_eq!(preset("crates.io"), Some(json!("package_registries")));
        let edit = Some(json!({"ports": [443, 8443], "version": 1}));
        assert_eq!(call(f, Method::PUT, "/api/v1/admin/egress/crates.io", &cookie, (edit, None)).await?.0, 200);
        assert_eq!(applied(f, &cookie).await?[0], ("package_registries".into(), "partial".into()));
        // Removing takes only the rules still tagged; the edited one and the administrator's own stay.
        let (status, removed) = call(f, Method::DELETE, &path, &cookie, (None, Some("remove-1"))).await?;
        assert_eq!(status, 200, "{removed}");
        assert_eq!(hosts(&removed, "kept"), ["crates.io"]);
        assert_eq!(hosts(&removed, "removed").len(), 6);
        let (_, list) = call(f, Method::GET, "/api/v1/admin/egress", &cookie, (None, None)).await?;
        assert_eq!(hosts(&list, "items"), ["crates.io", "pypi.org"]);
        // open_web stores the any-host rule, which can't be typed.
        let (status, open) = call(f, Method::POST, &format!("{PRESETS}/open_web"), &cookie, (None, Some("open-1"))).await?;
        assert_eq!((status, hosts(&open, "added")), (200, vec!["*".to_owned()]), "{open}");
        assert_eq!(open["added"][0]["ports"], json!([80, 443]));
        let (status, typed) = call(f, Method::PUT, "/api/v1/admin/egress/*", &cookie, (Some(json!({"ports": [443]})), None)).await?;
        assert_eq!(status, 400, "{typed}");
        // Only administrators, and only the built-in ids.
        assert_eq!(call(f, Method::POST, &path, &member, (None, Some("member-1"))).await?.0, 403);
        assert_eq!(call(f, Method::POST, &format!("{PRESETS}/everything"), &cookie, (None, Some("unknown-1"))).await?.0, 404);
        let audit: Vec<(String, String)> = sqlx::query_as(
            "SELECT action,item FROM admin_audit WHERE area='egress' AND actor_user_id=$1 AND item IN ('*','sum.golang.org') ORDER BY at, id",
        )
        .bind(admin.id)
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(audit, vec![("create".into(), "sum.golang.org".into()), ("delete".into(), "sum.golang.org".into()), ("create".into(), "*".into())]);
        Ok(())
    })
    .await
}
