//! GW-I-017: operational metrics are never served on the public listener.

#[path = "support/fixture.rs"]
mod support;
use sqlx::PgPool;
use support::{TestResult, run};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_017_public_listener_never_exposes_metrics(pool: PgPool) -> TestResult {
    run(pool, async |fixture| {
        let response = fixture.client.get(fixture.url("/metrics")).send().await?;
        assert_eq!(response.status(), 404);
        Ok(())
    })
    .await
}
