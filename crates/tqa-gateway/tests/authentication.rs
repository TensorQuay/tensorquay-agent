//! GW-I-001/GW-S-021: bootstrap, cookie sessions and origin checks over real HTTP.

#[path = "support/fixture.rs"]
mod support;
use serde_json::json;
use sqlx::PgPool;
use support::{TestResult, run};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_001_bootstrap_login_logout_and_password_policy(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let weak = fixture.post("/api/v1/auth/bootstrap", &json!({"email": "admin@example.invalid", "password": "password1234", "token": fixture.bootstrap()})).await?;
        assert_eq!(weak.status(), 400);
        let password = fixture.password();
        let response = fixture.post("/api/v1/auth/bootstrap", &json!({"email": "admin@example.invalid", "password": password, "token": fixture.bootstrap()})).await?;
        assert_eq!(response.status(), 201);
        let repeated = fixture.post("/api/v1/auth/bootstrap", &json!({"email": "next@example.invalid", "password": password, "token": fixture.bootstrap()})).await?;
        assert_eq!(repeated.status(), 409);
        let failed = fixture.post("/api/v1/auth/login", &json!({"email": "admin@example.invalid", "password": "incorrect-credential"})).await?;
        assert_eq!(failed.status(), 401);
        let response = fixture.post("/api/v1/auth/login", &json!({"email": "ADMIN@example.invalid", "password": password})).await?;
        assert_eq!(response.status(), 200);
        let cookie_header = response.headers().get("set-cookie").ok_or("missing session cookie")?.to_str()?;
        assert!(cookie_header.contains("HttpOnly"));
        assert!(cookie_header.contains("SameSite=Lax"));
        assert!(cookie_header.contains("Path=/"));
        let cookie = cookie_header.split(';').next().ok_or("missing cookie value")?.to_owned();
        let me = fixture.get("/api/v1/auth/me", &cookie).await?;
        assert_eq!(me.status(), 200);
        let body: serde_json::Value = me.json().await?;
        assert_eq!(body["role"], "admin");
        let logout = fixture.client.post(fixture.url("/api/v1/auth/logout")).header("origin", fixture.origin()).header("cookie", &cookie).send().await?;
        assert_eq!(logout.status(), 204);
        assert_eq!(fixture.get("/api/v1/auth/me", &cookie).await?.status(), 401);
        Ok(())
    }).await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_s_021_state_changes_reject_missing_and_foreign_origins(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let body = json!({"email": "admin@example.invalid", "password": fixture.password(), "token": fixture.bootstrap()});
        for origin in [None, Some("https://foreign.invalid")] {
            let mut request = fixture.client.post(fixture.url("/api/v1/auth/bootstrap")).json(&body);
            if let Some(origin) = origin { request = request.header("origin", origin); }
            assert_eq!(request.send().await?.status(), 403);
        }
        let response = fixture.client.post(fixture.url("/api/v1/auth/bootstrap")).header("sec-fetch-site", "same-origin").json(&body).send().await?;
        assert_eq!(response.status(), 201);
        Ok(())
    }).await
}
