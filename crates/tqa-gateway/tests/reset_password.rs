//! GW-I-390 (decided on 21 Nov 2026): `tqa-gateway reset-password` sets an active account's password from the host.
//! The person's sessions end and the new password signs in; a weak password and an unknown or disabled account are
//! refused and change nothing; the change is audited as the gateway's own, with no password in it.
#[path = "support/fixture.rs"]
mod support;
use secrecy::SecretString;
use sqlx::PgPool;
use support::TestResult;
use tokio_util::sync::CancellationToken;

// TEST: GW-I-390
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_390_an_administrator_resets_a_password_from_the_host(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let cancel = CancellationToken::new();
        let fresh = format!("a new long passphrase {}", uuid::Uuid::now_v7().simple());
        // Too weak, then an unknown account: refused, and the session still works.
        let weak = tqa_gateway::reset_password_in(&f.pool, f.config(), "admin@example.invalid", SecretString::from("short".to_owned()), &cancel).await;
        assert!(matches!(weak, Err(tqa_gateway::GatewayError::Invalid { field: "new_password", .. })), "{weak:?}");
        let unknown = tqa_gateway::reset_password_in(&f.pool, f.config(), "nobody@example.invalid", SecretString::from(fresh.clone()), &cancel).await;
        assert!(matches!(unknown, Err(tqa_gateway::GatewayError::UserNotFound)), "{unknown:?}");
        assert_eq!(f.get("/api/v1/auth/me", &cookie).await?.status(), 200);
        // The reset: the stored email back, the session ended, the new password signs in.
        let email = tqa_gateway::reset_password_in(&f.pool, f.config(), "ADMIN@example.invalid", SecretString::from(fresh.clone()), &cancel).await?;
        assert_eq!(email, "admin@example.invalid");
        assert_eq!(f.get("/api/v1/auth/me", &cookie).await?.status(), 401);
        let login = f
            .post("/api/v1/auth/login", &serde_json::json!({"email": "admin@example.invalid", "password": fresh}))
            .await?;
        assert_eq!(login.status(), 200);
        // Audited as the gateway's own change, naming the account, never the password.
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT actor_kind, area, item, action FROM admin_audit WHERE area = 'account' AND item = $1",
        )
        .bind(admin.id.to_string())
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(rows, [("system".to_owned(), "account".to_owned(), admin.id.to_string(), "replace".to_owned())]);
        // A disabled account can't be reset.
        sqlx::query("UPDATE users SET disabled_at = CURRENT_TIMESTAMP WHERE id = $1")
            .bind(admin.id)
            .execute(&f.pool)
            .await?;
        let disabled = tqa_gateway::reset_password_in(&f.pool, f.config(), "admin@example.invalid", SecretString::from(fresh), &cancel).await;
        assert!(matches!(disabled, Err(tqa_gateway::GatewayError::UserNotFound)), "{disabled:?}");
        Ok(())
    })
    .await
}
