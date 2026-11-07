//! GW-S-021: JSON, error and stream responses carry browser security and cache boundaries.

#[path = "support/fixture.rs"]
mod support;
use sqlx::PgPool;
use support::{TestResult, run};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_s_021_security_headers_cover_json_errors_and_streams(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let (user, cookie) = fixture.admin().await?;
        let conversation = fixture.conversation(user.id).await?;
        for (path, status) in [
            ("/api/v1/auth/me".to_owned(), 200),
            ("/api/v1/conversations/invalid".to_owned(), 400),
            (
                format!(
                    "/api/v1/conversations/{}/events?stream=1&protocol=1",
                    conversation.id
                ),
                200,
            ),
        ] {
            let response = fixture.get(&path, &cookie).await?;
            assert_eq!(response.status(), status);
            assert_eq!(
                response
                    .headers()
                    .get("x-content-type-options")
                    .ok_or("missing nosniff")?,
                "nosniff"
            );
            assert_eq!(
                response
                    .headers()
                    .get("cache-control")
                    .ok_or("missing no-store")?,
                "no-store"
            );
        }
        assert_eq!(
            fixture
                .client
                .get(fixture.url("/metrics"))
                .send()
                .await?
                .status(),
            404
        );
        Ok(())
    })
    .await
}
