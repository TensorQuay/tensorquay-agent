//! GW-I-063: sandboxes get the install skills tier and the conversation owner's own user tier, never a sharer's; a
//! microVM install sends no skills (its backend has no read-only tree yet).
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::ConversationId;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn skills_config(root: &std::path::Path) -> String {
    format!("[skills]\nroot={}\n", toml_path(root))
}

fn toml_path(path: &std::path::Path) -> String {
    format!("'{}'", path.display())
}

async fn call(f: &Fixture, cookie: &str, method: Method, path: &str, body: Value) -> Result<Value> {
    let response = f
        .client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .header("idempotency-key", uuid::Uuid::now_v7().to_string())
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    assert!(status.is_success(), "{path}: {status} {body}");
    Ok(body)
}

async fn task(f: &Fixture, cookie: &str, workspace: &str) -> Result<ConversationId> {
    let body = json!({"title":"Task","model_route":"local","workspace_id":workspace});
    let created = call(f, cookie, Method::POST, "/api/v1/conversations", body).await?;
    Ok(created["id"].as_str().ok_or("task id")?.parse()?)
}

fn user_tier(f: &Fixture, user: uuid::Uuid) -> std::path::PathBuf {
    f.config_storage_root()
        .join("skills")
        .join("users")
        .join(user.to_string())
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_063_each_conversation_mounts_its_owners_tier_and_the_install_tier(
    pool: PgPool,
) -> TestResult {
    let install = tempfile::tempdir()?;
    let canonical = install.path().canonicalize()?;
    support::run_configured(pool, None, &skills_config(install.path()), async |f| {
        let (owner, owner_cookie) = f.admin().await?;
        let (sharer, sharer_cookie) = f.member("sharer@example.invalid").await?;
        let body = json!({"name":"Shared"});
        let created = call(f, &owner_cookie, Method::POST, "/api/v1/workspaces", body).await?;
        let workspace = created["id"].as_str().ok_or("workspace id")?.to_owned();
        let share = format!("/api/v1/workspaces/{workspace}/shares/{}", sharer.id);
        call(
            f,
            &owner_cookie,
            Method::PUT,
            &share,
            json!({"access":"write"}),
        )
        .await?;
        // The sharer's task runs in the owner's workspace; the owner's own task runs in the owner's default workspace,
        // since a workspace has one live sandbox at a time (GW-I-031).
        let shared = task(f, &sharer_cookie, &workspace).await?;
        let owned = f.conversation(owner.id).await?.id;
        for (conversation, user) in [(&shared, sharer.id), (&owned, owner.id)] {
            f.gateway.sandboxes().ensure(conversation).await?;
            let skills = f
                .backend
                .skills(conversation)
                .ok_or("no sandbox was created")?;
            assert_eq!(skills.install.as_deref(), Some(canonical.as_path()));
            let tier = skills.user.ok_or("no user tier")?;
            assert_eq!(
                tier,
                user_tier(f, user),
                "the conversation owner's own tier"
            );
            let mode =
                std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&tier)?.permissions());
            assert_eq!(
                mode & 0o777,
                0o700,
                "a private directory, created on first use"
            );
        }
        assert_ne!(user_tier(f, owner.id), user_tier(f, sharer.id));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_063_without_an_install_tier_only_the_user_tier_is_mounted(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        f.gateway.sandboxes().ensure(&conversation.id).await?;
        let skills = f
            .backend
            .skills(&conversation.id)
            .ok_or("no sandbox was created")?;
        assert_eq!(skills.install, None);
        assert_eq!(skills.user, Some(user_tier(f, user.id)));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_063_a_microvm_install_sends_no_skills(pool: PgPool) -> TestResult {
    let install = tempfile::tempdir()?;
    let config = format!(
        "{}[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\n",
        skills_config(install.path()),
        "b".repeat(64)
    );
    support::run_configured(pool, None, &config, async |f| {
        let (user, _) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        f.gateway.sandboxes().ensure(&conversation.id).await?;
        let skills = f
            .backend
            .skills(&conversation.id)
            .ok_or("no sandbox was created")?;
        assert!(skills.is_empty(), "{skills:?}");
        assert!(!f.config_storage_root().join("skills").exists());
        Ok(())
    })
    .await
}
