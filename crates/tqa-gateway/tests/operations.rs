//! GW-I-017: process health and database readiness have separate failure boundaries.

#[path = "support/fixture.rs"]
mod support;
use sqlx::PgPool;
use support::{TestResult, run};
use tower::ServiceExt;

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_017_health_readiness_and_metrics_reflect_database_availability(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        assert_eq!(
            fixture
                .client
                .get(fixture.url("/healthz"))
                .send()
                .await?
                .status(),
            200
        );
        assert_eq!(
            fixture
                .client
                .get(fixture.url("/readyz"))
                .send()
                .await?
                .status(),
            200
        );
        assert_eq!(
            fixture
                .client
                .get(fixture.url("/metrics"))
                .send()
                .await?
                .status(),
            404
        );
        let metrics = fixture
            .gateway
            .metrics_router()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/metrics")
                    .body(axum::body::Body::empty())?,
            )
            .await?;
        assert_eq!(metrics.status(), 200);
        let metrics = String::from_utf8(
            axum::body::to_bytes(metrics.into_body(), 1024 * 1024)
                .await?
                .to_vec(),
        )?;
        assert!(metrics.contains("tqa_http_requests_total"));
        assert!(metrics.contains("tqa_http_request_duration_seconds"));
        assert!(metrics.contains("# TYPE tqa_http_request_duration_seconds histogram"));
        fixture.pool.close().await;
        assert_eq!(
            fixture
                .client
                .get(fixture.url("/healthz"))
                .send()
                .await?
                .status(),
            200
        );
        assert_eq!(
            fixture
                .client
                .get(fixture.url("/readyz"))
                .send()
                .await?
                .status(),
            503
        );
        Ok(())
    })
    .await
}
