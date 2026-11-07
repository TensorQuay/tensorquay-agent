//! GW-I-017: operational detail is an administrator-only capability report.
#[path = "support/fixture.rs"]
mod support;
use sqlx::PgPool;
use support::{TestResult, run};

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_017_readiness_detail_requires_admin_and_checks_dependencies(
    pool: PgPool,
) -> TestResult {
    run(pool, async |fixture| {
        let (_, admin) = fixture.admin().await?;
        let (_, member) = fixture.member("readiness@example.invalid").await?;
        assert_eq!(fixture.get("/readyz?detail=1", "").await?.status(), 401);
        assert_eq!(
            fixture.get("/readyz?detail=1", &member).await?.status(),
            403
        );
        let response = fixture.get("/readyz?detail=1", &admin).await?;
        assert_eq!(response.status(), 200);
        let detail: serde_json::Value = response.json().await?;
        assert_eq!(detail["ready"], true);
        for check in [
            "database",
            "secret_store",
            "backend_preflight",
            "image_present",
        ] {
            assert_eq!(detail["checks"][check], true, "{check}");
        }
        // GW-I-229: no runtime linked, so no task is mid-step (a count, never which tasks).
        assert_eq!(detail["runs_active"], 0);
        assert_eq!(detail["sandbox"]["network_mode"], "internal");
        assert_eq!(detail["sandbox"]["backend"], "docker");
        // The documented ReadyDetail schema (openapi.json) names exactly these fields, so the web's generated type matches.
        let document: serde_json::Value = serde_json::from_str(include_str!("../openapi.json"))?;
        let schema = &document["components"]["schemas"]["ReadyDetail"];
        let keys = |value: &serde_json::Value| {
            value.as_object().map(|object| {
                object
                    .keys()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>()
            })
        };
        let required = |value: &serde_json::Value| {
            value.as_array().map(|names| {
                names
                    .iter()
                    .filter_map(|name| name.as_str().map(str::to_owned))
                    .collect::<std::collections::BTreeSet<_>>()
            })
        };
        assert_eq!(keys(&detail), required(&schema["required"]));
        assert_eq!(
            keys(&detail["checks"]),
            required(&schema["properties"]["checks"]["required"])
        );
        assert_eq!(
            keys(&detail["sandbox"]),
            required(&schema["properties"]["sandbox"]["required"])
        );
        assert_eq!(fixture.get("/readyz?detail=2", &admin).await?.status(), 400);
        assert_eq!(fixture.get("/readyz", "").await?.status(), 200);
        Ok(())
    })
    .await
}
