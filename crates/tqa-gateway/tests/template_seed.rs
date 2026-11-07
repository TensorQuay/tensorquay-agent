//! GW-I-075 (ADR-0028, ADR-0032): environment templates live in the database, seeded from `[[templates]]`; a seed
//! template applies only when it changed since it was last imported, a dropped one is kept and marked, and the rows of
//! an install from before this rule are adopted without churn.
#[path = "support/fixture.rs"]
mod support;

use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::fmt::Write as _;
use support::{Fixture, TestResult};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
/// One audit row: actor kind, action, the versions before and after, and whether the value changed.
type AuditRow = (String, String, Option<i64>, Option<i64>, bool);

const PIN: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

/// `[[templates]]` entries: the default Workstation with `display`, plus each extra name.
fn templates(display: &str, extra: &[&str]) -> String {
    let mut text = format!(
        "[[templates]]\nname='workstation'\ndisplay_name='{display}'\nimage='{PIN}'\ncapabilities=['shell']\ndefault=true\n"
    );
    for name in extra {
        let _ = write!(
            text,
            "\n[[templates]]\nname='{name}'\ndisplay_name='{name}'\nimage='{PIN}'\ncapabilities=['shell']\n"
        );
    }
    text
}

/// The `version`, `last_set_by` and `seed_state` of one template.
async fn provenance(pool: &PgPool, name: &str) -> Result<(i64, String, Option<String>)> {
    let row = sqlx::query("SELECT version,last_set_by,seed_state FROM templates WHERE name=$1")
        .bind(name)
        .fetch_one(pool)
        .await?;
    Ok((row.try_get(0)?, row.try_get(1)?, row.try_get(2)?))
}

/// The display name stored for `name` (offering is GW-I-076's subject; this test is about the stored values).
async fn shown(pool: &PgPool, name: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT display_name FROM templates WHERE name=$1")
            .bind(name)
            .fetch_optional(pool)
            .await?,
    )
}

async fn restart_with(f: &mut Fixture, config: &str) -> TestResult {
    f.reconfigure(config)?;
    f.restart().await
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_075_an_unchanged_seed_keeps_an_admin_change_and_an_edited_seed_wins(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, &templates("Workstation", &[]), async |f| {
        f.admin().await?;
        assert_eq!(provenance(&f.pool, "workstation").await?, (1, "seed".into(), Some("current".into())));
        // An administrator's change, as the templates API will write it.
        sqlx::query("UPDATE templates SET display_name='Admin', version=version+1, last_set_by='admin' WHERE name='workstation'")
            .execute(&f.pool)
            .await?;
        restart_with(f, &templates("Workstation", &[])).await?;
        assert_eq!(shown(&f.pool, "workstation").await?.as_deref(), Some("Admin"), "the same seed keeps it");
        assert_eq!(provenance(&f.pool, "workstation").await?, (2, "admin".into(), Some("current".into())));
        restart_with(f, &templates("Workstation 2", &[])).await?;
        assert_eq!(shown(&f.pool, "workstation").await?.as_deref(), Some("Workstation 2"), "an edited seed wins");
        assert_eq!(provenance(&f.pool, "workstation").await?, (3, "seed".into(), Some("current".into())));
        let audit: Vec<AuditRow> = sqlx::query_as(
            "SELECT actor_kind,action,before_version,after_version,value_hash_changed FROM admin_audit WHERE area='templates' AND item='workstation' ORDER BY at, id",
        )
        .fetch_all(&f.pool)
        .await?;
        assert_eq!(
            audit,
            vec![
                ("seed".into(), "create".into(), None, Some(1), true),
                ("seed".into(), "replace".into(), Some(2), Some(3), true)
            ]
        );
        let values: String = sqlx::query_scalar("SELECT coalesce(string_agg(row_to_json(a)::text, ''), '') FROM admin_audit a WHERE area='templates'")
            .fetch_one(&f.pool)
            .await?;
        assert!(!values.contains(PIN) && !values.contains("Workstation"), "no values in the audit");
        Ok(())
    })
    .await
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_075_a_template_dropped_from_the_seed_is_kept_marked_and_still_bound(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(
        pool,
        None,
        &templates("Workstation", &["extra"]),
        async |f| {
            let (_, cookie) = f.admin().await?;
            let created: Value = f
                .action(
                    "/api/v1/conversations",
                    &cookie,
                    Some("bound"),
                    &json!({"title":"Bound","model_route":"local","template":"extra"}),
                )
                .await?
                .error_for_status()?
                .json()
                .await?;
            let id = created["id"].as_str().ok_or("id")?.to_owned();
            restart_with(f, &templates("Workstation", &[])).await?;
            assert_eq!(
                provenance(&f.pool, "extra").await?,
                (1, "seed".into(), Some("removed".into()))
            );
            let conversation: Value = f
                .get(&format!("/api/v1/conversations/{id}"), &cookie)
                .await?
                .error_for_status()?
                .json()
                .await?;
            assert_eq!(
                conversation["template"], "extra",
                "the bound conversation keeps its template"
            );
            let (template, digest): (String, String) = sqlx::query_as(
                "SELECT template,template_digest FROM conversations WHERE id=$1::uuid",
            )
            .bind(&id)
            .fetch_one(&f.pool)
            .await?;
            assert_eq!((template.as_str(), digest.as_str()), ("extra", PIN));
            // Listed again, it is current again without a new version.
            restart_with(f, &templates("Workstation", &["extra"])).await?;
            assert_eq!(
                provenance(&f.pool, "extra").await?,
                (1, "seed".into(), Some("current".into()))
            );
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_075_rows_from_before_the_seed_rule_are_adopted_without_churn(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, &templates("Workstation", &[]), async |f| {
        // As an install from before this rule left it: the seeded values, no provenance, no audit.
        sqlx::query(
            "UPDATE templates SET seed_digest=NULL, seed_state=NULL WHERE name='workstation'",
        )
        .execute(&f.pool)
        .await?;
        sqlx::query("DELETE FROM admin_audit WHERE area='templates'")
            .execute(&f.pool)
            .await?;
        restart_with(f, &templates("Workstation", &[])).await?;
        assert_eq!(
            provenance(&f.pool, "workstation").await?,
            (1, "seed".into(), Some("current".into()))
        );
        let audited: i64 =
            sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE area='templates'")
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(audited, 0, "adopting the same values is not a change");
        Ok(())
    })
    .await
}
