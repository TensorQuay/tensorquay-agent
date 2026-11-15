//! GW-I-325 (the third 24 h soak, 16 Nov 2026): a task deleted while the container engine fails its destroys keeps no
//! sandbox. The soak's Desktop task was deleted during an engine stall: the delete's teardown failed, the failure
//! path's teardown failed too, and nothing tried again, so the deleted task's sandbox, its screenshots and its browser
//! profile outlived it ("delete means gone"). The teardown is now retried in the background until it succeeds, and the
//! purge then finds the task without a sandbox.

#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use sqlx::PgPool;
use std::{sync::atomic::Ordering, time::Duration};
use support::{TestResult, call};
use tqa_sandbox::SandboxBackend;

// TEST: GW-I-325
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_325_a_task_deleted_while_destroys_fail_keeps_no_sandbox(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "", async |f| {
        let (user, cookie) = f.admin().await?;
        let deleted = f.conversation(user.id).await?;
        let runtime = f.runtime(&deleted).await?;
        f.clock.freeze();
        f.clock.observed().await?;
        // The engine refuses the next two destroys: the delete's own and the failure path's.
        f.backend.fail_destroys.store(2, Ordering::SeqCst);
        let path = format!("/api/v1/conversations/{}", deleted.id);
        assert_eq!(
            call(f, Method::DELETE, &path, &cookie, (None, None))
                .await?
                .0,
            204
        );
        drop(runtime);
        // The engine is back; the background retry removes the sandbox within its first waits.
        tokio::time::timeout(support::DEADLINE, async {
            while !f.backend.list().await?.is_empty() {
                f.clock.advance(Duration::from_secs(15));
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await
        .map_err(|_| "the deleted task's sandbox was never removed")??;
        assert!(
            f.backend.destroys.load(Ordering::SeqCst) >= 3,
            "removed by a retry after the two refused destroys"
        );
        Ok(())
    })
    .await
}
