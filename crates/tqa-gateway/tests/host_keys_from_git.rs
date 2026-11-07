//! GW-I-164 (ADR-0037 §7): at its start the gateway copies each Git credential into its owner's own host key, `basic`
//! with its user or `bearer`, for the remote's host and path, the same value under a host key's reference, and "check
//! the name" set, which any successful save clears (the same name included). Since the second release it then removes
//! the original and its sealed value, so an install upgrading from before the copy existed gets its keys at the first
//! start; migration 0088 removes the originals an earlier start had copied. One that can't become a key is kept, and
//! the administrator's readiness detail counts it. The Git credentials' routes are gone.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use secrecy::ExposeSecret as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{TestResult, call};

const GITHUB: &str = "tqa-test-git-github-0a1b";
const GITLAB: &str = "tqa-test-git-gitlab-2c3d";

fn git_ref(n: u8) -> String {
    format!(
        "secrets/git-{}",
        format!("{n:x}")
            .repeat(64)
            .chars()
            .take(64)
            .collect::<String>()
    )
}

async fn keys(
    f: &support::Fixture,
    cookie: &str,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let (status, listed) = call(f, Method::GET, "/api/v1/host-keys", cookie, (None, None)).await?;
    assert_eq!(status, 200, "{listed}");
    Ok(listed["items"].as_array().cloned().unwrap_or_default())
}

/// A member with a key that already has the first credential's name and two Git credentials with values, as a release
/// before the copy left them; then the gateway restarts. The member and their cookie.
async fn upgraded(
    f: &mut support::Fixture,
) -> Result<(tqa_gateway::User, String), Box<dyn std::error::Error>> {
    let (_, _admin) = f.admin().await?;
    let (member, cookie) = f.member("dev@example.invalid").await?;
    let body = json!({"name": "GITHUB_ACME_TOOLS", "address": "https://api.github.com", "scheme": "bearer", "value": "x"});
    let (status, _) = call(
        f,
        Method::POST,
        "/api/v1/host-keys",
        &cookie,
        (Some(body), Some("own")),
    )
    .await?;
    assert_eq!(status, 201);
    for (n, remote, kind, user, value) in [
        (
            1,
            "https://github.com/acme/tools.git",
            "basic",
            Some("x-access-token"),
            GITHUB,
        ),
        (2, "https://gitlab.com/group/repo", "bearer", None, GITLAB),
    ] {
        sqlx::query("INSERT INTO git_credentials (user_id, remote, kind, username, secret_ref) VALUES ($1,$2,$3,$4,$5)")
            .bind(member.id)
            .bind(remote)
            .bind(kind)
            .bind(user)
            .bind(git_ref(n))
            .execute(&f.pool)
            .await?;
        f.gateway
            .secret_store()
            .put(&git_ref(n).parse()?, value.to_owned().into())
            .await?;
    }
    f.restart().await?;
    Ok((member, cookie))
}

/// The value of the member's copied key for `host`, from the store, after checking it is under a host key's reference.
async fn sealed(
    f: &support::Fixture,
    member: uuid::Uuid,
    host: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let reference: String = sqlx::query_scalar(
        "SELECT secret_ref FROM host_keys WHERE owner_id=$1 AND host=$2 AND check_name",
    )
    .bind(member)
    .bind(host)
    .fetch_one(&f.pool)
    .await?;
    assert!(reference.starts_with("secrets/host-"), "{reference}");
    let value = f
        .gateway
        .secret_store()
        .get(&reference.parse()?)
        .await?
        .ok_or("value")?;
    Ok(value.expose_secret().to_owned())
}

// TEST: GW-I-164
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_164_git_credentials_become_the_owners_keys_named_to_check(
    pool: PgPool,
) -> TestResult {
    support::run_restarting(pool, async |f| {
        let (member, cookie) = upgraded(f).await?;
        let listed = keys(f, &cookie).await?;
        let copied: Vec<&Value> = listed
            .iter()
            .filter(|key| key["check_name"] == json!(true))
            .collect();
        assert_eq!(copied.len(), 2, "{listed:?}");
        let github = copied
            .iter()
            .find(|key| key["host"] == "github.com")
            .ok_or("github")?;
        assert_eq!(
            (
                &github["name"],
                &github["scheme"],
                &github["user_name"],
                &github["path_prefix"],
                &github["port"]
            ),
            (
                &json!("GITHUB_ACME_TOOLS_2"),
                &json!("basic"),
                &json!("x-access-token"),
                &json!("/acme/tools.git"),
                &json!(443)
            ),
            "{github}"
        );
        let gitlab = copied
            .iter()
            .find(|key| key["host"] == "gitlab.com")
            .ok_or("gitlab")?;
        assert_eq!(
            (&gitlab["name"], &gitlab["scheme"], &gitlab["user_name"]),
            (&json!("GITLAB_GROUP_REPO"), &json!("bearer"), &Value::Null)
        );
        // The same values, under host keys' own references.
        assert_eq!(sealed(f, member.id, "github.com").await?, GITHUB);
        assert_eq!(sealed(f, member.id, "gitlab.com").await?, GITLAB);
        Ok(())
    })
    .await
}

// TEST: GW-I-164
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_164_from_before_the_copy_each_original_goes_once_its_key_is_made(
    pool: PgPool,
) -> TestResult {
    support::run_restarting(pool, async |f| {
        // As an install upgrading straight from a release before the copy: the credentials were never copied.
        let (member, cookie) = upgraded(f).await?;
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM git_credentials")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(left, 0, "each original goes once its key is committed");
        for n in [1, 2] {
            assert!(f.gateway.secret_store().get(&git_ref(n).parse()?).await?.is_none(), "value {n} removed");
        }
        assert_eq!(sealed(f, member.id, "github.com").await?, GITHUB);
        // The routes are gone with the relay.
        for (method, path) in [
            (Method::GET, "/api/v1/git/credentials"),
            (Method::PUT, "/api/v1/git/credentials"),
            (Method::DELETE, "/api/v1/git/credentials"),
            (Method::GET, "/api/v1/git/credentials/reads"),
        ] {
            let body = (method == Method::PUT).then(|| json!({"remote": "https://github.com/acme/new.git"}));
            let (status, _) = call(f, method.clone(), path, &cookie, (body, None)).await?;
            assert_eq!(status, 404, "{method} {path}");
        }
        // A second start copies nothing twice; saving a copied key with the same name clears "check the name".
        f.restart().await?;
        let listed = keys(f, &cookie).await?;
        assert_eq!(listed.len(), 3);
        let github = listed.iter().find(|key| key["name"] == "GITHUB_ACME_TOOLS_2").ok_or("github")?;
        let save = json!({"version": github["version"], "name": "GITHUB_ACME_TOOLS_2", "address": "https://github.com/acme/tools.git",
                          "scheme": "basic", "user_name": "x-access-token"});
        let id = github["id"].as_str().ok_or("id")?;
        let (status, saved) = call(f, Method::PUT, &format!("/api/v1/host-keys/{id}"), &cookie, (Some(save), Some("save"))).await?;
        assert_eq!((status, &saved["check_name"]), (200, &json!(false)), "{saved}");
        Ok(())
    })
    .await
}

// TEST: GW-I-164
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_164_the_upgrade_removes_what_the_last_release_copied_and_keeps_the_rest(
    pool: PgPool,
) -> TestResult {
    support::run_restarting(pool, async |f| {
        let (_, _admin) = f.admin().await?;
        let (member, _) = f.member("dev@example.invalid").await?;
        // As the last release left them: one copied (moved_at set), one not yet.
        for (n, remote, moved) in [(1, "https://github.com/acme/tools.git", true), (2, "https://gitlab.com/group/repo", false)] {
            sqlx::query(
                "INSERT INTO git_credentials (user_id, remote, kind, secret_ref, moved_at) VALUES ($1,$2,'bearer',$3, \
                 CASE WHEN $4 THEN now() END)",
            )
            .bind(member.id)
            .bind(remote)
            .bind(git_ref(n))
            .bind(moved)
            .execute(&f.pool)
            .await?;
            f.gateway.secret_store().put(&git_ref(n).parse()?, "v".to_owned().into()).await?;
        }
        sqlx::raw_sql(include_str!("../migrations/0088_git_credentials_removed.sql"))
            .execute(&f.pool)
            .await?;
        let remotes: Vec<String> = sqlx::query_scalar("SELECT remote FROM git_credentials")
            .fetch_all(&f.pool)
            .await?;
        assert_eq!(remotes, ["https://gitlab.com/group/repo"], "the copied one goes, the other waits for the copy");
        assert!(f.gateway.secret_store().get(&git_ref(1).parse()?).await?.is_none(), "its sealed value too");
        assert!(f.gateway.secret_store().get(&git_ref(2).parse()?).await?.is_some(), "not the other's");
        Ok(())
    })
    .await
}

// TEST: GW-I-164
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_164_one_that_cant_become_a_key_is_kept_and_counted_for_the_administrator(
    pool: PgPool,
) -> TestResult {
    support::run_restarting(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (member, _) = f.member("dev@example.invalid").await?;
        // An SSH-style remote: no key can have it as its address.
        sqlx::query("INSERT INTO git_credentials (user_id, remote, kind, secret_ref) VALUES ($1,$2,'bearer',$3)")
            .bind(member.id)
            .bind("git@github.com:acme/tools.git")
            .bind(git_ref(3))
            .execute(&f.pool)
            .await?;
        f.gateway.secret_store().put(&git_ref(3).parse()?, "v".to_owned().into()).await?;
        f.restart().await?;
        let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM git_credentials WHERE moved_at IS NULL")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(kept, 1, "kept, unused");
        assert!(f.gateway.secret_store().get(&git_ref(3).parse()?).await?.is_some(), "with its value");
        let (_, detail) = call(f, Method::GET, "/readyz?detail=1", &admin, (None, None)).await?;
        assert_eq!(
            detail["git_credentials_kept"],
            json!({"not_web_address": 1, "waiting": 0}),
            "{detail}"
        );
        Ok(())
    })
    .await
}
