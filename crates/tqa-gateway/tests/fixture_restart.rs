//! The test fixture's restart: one that fails after the old server ended reports its own error, and the clean-up after
//! it never awaits that finished server again ("`JoinHandle` polled after completion" hid the error in GW-I-129's restart
//! tests under load).
#[path = "support/fixture.rs"]
mod support;

use sqlx::PgPool;
use support::TestResult;

// TEST: GW-I-209
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_209_a_restart_that_fails_after_the_old_server_ended_reports_its_own_error(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, "", async |f| {
        // A seed file the start-up can't read: the new gateway fails to start, after the old server has ended.
        f.seed_secrets("this is [not toml")?;
        let failed = f.restart().await;
        let error = failed
            .err()
            .ok_or("a restart with an unreadable seed file started")?;
        assert!(
            !error.to_string().is_empty(),
            "the restart names its failure"
        );
        Ok(())
    })
    .await
}
