//! GW-I-142: the web learns the installation's upload limits from `GET /api/v1/auth/me`, which it reads at start, so a
//! file over them is refused in the browser before anything is sent.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::TestResult;

const MIB: u64 = 1024 * 1024;

/// The limits `me` answers, after checking that signing in answers the same (the web may start from either).
async fn limits(f: &support::Fixture) -> Result<Value, Box<dyn std::error::Error>> {
    let (_, cookie) = f.admin().await?;
    let me: Value = f.get("/api/v1/auth/me", &cookie).await?.json().await?;
    let signed_in: Value = f
        .post(
            "/api/v1/auth/login",
            &json!({"email": "admin@example.invalid", "password": f.password()}),
        )
        .await?
        .json()
        .await?;
    assert_eq!(signed_in["limits"], me["limits"], "sign-in and me agree");
    Ok(me["limits"].clone())
}

// TEST: GW-I-142
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_142_me_carries_the_default_upload_limits(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        assert_eq!(
            limits(f).await?,
            json!({"upload_bytes": 25 * MIB, "uploads_bytes": 1024 * MIB, "preview_html_bytes": 2 * MIB,
                   "preview_pdf_bytes": 20 * MIB, "preview_sheet_bytes": 10 * MIB})
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-142
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_142_me_carries_a_smaller_configured_upload_limit(pool: PgPool) -> TestResult {
    let config = format!(
        "[files]\nupload_bytes={}\nuploads_bytes={}\n",
        2 * MIB,
        64 * MIB
    );
    support::run_configured(pool, None, &config, async |f| {
        assert_eq!(
            limits(f).await?,
            json!({"upload_bytes": 2 * MIB, "uploads_bytes": 64 * MIB, "preview_html_bytes": 2 * MIB,
                   "preview_pdf_bytes": 20 * MIB, "preview_sheet_bytes": 10 * MIB})
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-144
// The file previews design: the web learns each preview limit from the same answer, so it says a file is too large to
// preview before fetching it.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_144_me_carries_the_configured_preview_limits(pool: PgPool) -> TestResult {
    let config = format!(
        "[files]\npreview_html_bytes={}\npreview_pdf_bytes={}\npreview_sheet_bytes={}\n",
        MIB,
        5 * MIB,
        3 * MIB
    );
    support::run_configured(pool, None, &config, async |f| {
        let limits = limits(f).await?;
        assert_eq!(
            (
                limits["preview_html_bytes"].clone(),
                limits["preview_pdf_bytes"].clone(),
                limits["preview_sheet_bytes"].clone()
            ),
            (json!(MIB), json!(5 * MIB), json!(3 * MIB))
        );
        Ok(())
    })
    .await
}
