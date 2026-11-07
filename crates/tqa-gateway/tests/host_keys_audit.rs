//! GW-I-167 (ADR-0037 §8): a host key's use writes its row before the value is read, counted per key, host, run and
//! outcome and attributed to the caller; a use whose row can't be written is refused (`unrecorded`) and no value is
//! read; a refused use is recorded with its reason.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use support::{TestResult, call};

type Row = (String, String, Option<String>, String, i64);

async fn rows(pool: &PgPool) -> Result<Vec<Row>, sqlx::Error> {
    sqlx::query_as(
        "SELECT consumer_kind, consumer, run_id, outcome, count FROM secret_reads WHERE consumer_kind='host' \
         ORDER BY outcome, run_id",
    )
    .fetch_all(pool)
    .await
}

// TEST: GW-I-167
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_167_a_keys_use_is_recorded_before_its_value_is_read(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let body = json!({"name": "GITHUB_WORK", "address": "https://api.github.com", "scheme": "bearer",
                          "value": "tqa-test-host-key-audit-4d2"});
        let (status, added) = call(f, Method::POST, "/api/v1/host-keys", &cookie, (Some(body), Some("add"))).await?;
        assert_eq!(status, 201, "{added}");
        let reference: String = sqlx::query_scalar("SELECT secret_ref FROM host_keys").fetch_one(&f.pool).await?;
        let task = f.conversation(owner.id).await?;
        let task = uuid::Uuid::parse_str(task.id.as_str())?;
        let at = ("api.github.com", 443);

        // Each use returns only once its row is committed: the row is there at once, counted per run.
        for _ in 0..3 {
            f.gateway.host_key_use(&reference, at, (task, owner.id, "run-1")).await.map_err(ToOwned::to_owned)?;
            assert!(!rows(&f.pool).await?.is_empty(), "the row is written before the value is used");
        }
        f.gateway.host_key_use(&reference, at, (task, owner.id, "run-2")).await.map_err(ToOwned::to_owned)?;
        // A refusal is recorded with its reason, and the value is never read for it.
        f.gateway.host_key_refusal(&reference, at, (task, owner.id, "run-2"), "wrong_host").await;
        f.gateway.read_audit_flush_now().await?;
        let host = || ("host".to_owned(), "api.github.com:443".to_owned());
        assert_eq!(
            rows(&f.pool).await?,
            [
                (host().0, host().1, Some("run-1".to_owned()), "ok".to_owned(), 3),
                (host().0, host().1, Some("run-2".to_owned()), "ok".to_owned(), 1),
                (host().0, host().1, Some("run-2".to_owned()), "wrong_host".to_owned(), 1),
            ]
        );
        let attributed: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM secret_reads WHERE consumer_kind='host' AND conversation_id=$1 AND user_id=$2",
        )
        .bind(task)
        .bind(owner.id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(attributed, 3);

        // The audit can't be written: the use is refused as unrecorded, and its value isn't read.
        sqlx::query("ALTER TABLE secret_reads RENAME TO secret_reads_away").execute(&f.pool).await?;
        let refused = f.gateway.host_key_use(&reference, at, (task, owner.id, "run-3")).await;
        sqlx::query("ALTER TABLE secret_reads_away RENAME TO secret_reads").execute(&f.pool).await?;
        assert_eq!(refused, Err("unrecorded"));
        let after = rows(&f.pool).await?;
        assert!(after.iter().all(|row| row.2.as_deref() != Some("run-3")), "{after:?}");
        Ok(())
    })
    .await
}
