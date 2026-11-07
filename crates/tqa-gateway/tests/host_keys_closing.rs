//! GW-I-180 (§19, a defect of the keyed tunnel's actor): a keyed host that closes its connection after answering
//! (`Connection: close`, as every HTTP/1.0 server does) still has its answer delivered to the sandbox, whole; one that
//! drops an idle connection gets the next request refused in words; one that cuts its answer short ends the read in an
//! error, never a reply that looks whole.
#[path = "support/keyed.rs"]
mod keyed;
#[path = "support/fixture.rs"]
mod support;

use http_body_util::BodyExt as _;
use keyed::*;
use sqlx::PgPool;
use support::TestResult;

// TEST: GW-I-180
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_180_an_answer_from_a_host_that_closes_after_it_arrives_whole(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        f.gateway.trust_upstream_root(&hosts.ca)?;
        let run = started_with(f, &hosts, &[("CLOSING_KEY", "https://close.keyed.test")]).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        let mut client = keyed_client(run.proxy, CLOSING, run.install.clone()).await?;
        // The host answers and closes: the answer still reaches the sandbox, whole and scrubbed.
        let (header, body) = echo(&mut client, get(CLOSING, "/echo", Some(&bearer))?).await?;
        assert_eq!(
            (header.as_str(), body),
            (bearer.as_str(), format!("you sent {bearer}"))
        );
        // That tunnel's host is gone: a later request on it is answered in words, never sent, or the connection ends.
        let again = client
            .send_request(get(CLOSING, "/echo", Some(&bearer))?)
            .await;
        if let Ok(response) = again {
            let status = response.status().as_u16();
            let words = String::from_utf8_lossy(&response.into_body().collect().await?.to_bytes())
                .into_owned();
            assert!(
                status == 502 && words.contains("closed"),
                "{status} {words}"
            );
        }
        let seen = hosts.closing.0.lock().map_err(|_| "lock")?.len();
        assert_eq!(seen, 1, "the host got the first request only");
        Ok(())
    })
    .await
}

// TEST: GW-I-180
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_180_an_answer_cut_short_by_the_host_ends_the_connection_never_looks_whole(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        f.gateway.trust_upstream_root(&hosts.ca)?;
        let run = started_with(f, &hosts, &[("SHORT_KEY", "https://short.keyed.test")]).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        let mut client = keyed_client(run.proxy, SHORT, run.install.clone()).await?;
        let response = client
            .send_request(get(SHORT, "/echo", Some(&bearer))?)
            .await?;
        assert_eq!(response.status(), 200, "the head came whole");
        // The body never completes: reading it ends in an error, never a shorter reply that looks whole.
        let body = tokio::time::timeout(support::DEADLINE, response.into_body().collect()).await?;
        assert!(
            body.is_err(),
            "the cut answer must not look complete: {:?}",
            body.map(http_body_util::Collected::to_bytes)
        );
        assert_eq!(hosts.short.load(std::sync::atomic::Ordering::SeqCst), 1);
        Ok(())
    })
    .await
}

// TEST: GW-I-180
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_180_after_a_host_drops_an_idle_connection_a_later_request_is_refused_in_words(
    pool: PgPool,
) -> TestResult {
    let hosts = hosts().await?;
    support::run_configured(pool, None, &config(5), async |f| {
        f.gateway.trust_upstream_root(&hosts.ca)?;
        let run = started_with(f, &hosts, &[("OLD_KEY", "https://old.keyed.test")]).await?;
        let bearer = format!("Bearer {}", run.placeholder);
        let mut client = keyed_client(run.proxy, OLD, run.install.clone()).await?;
        assert_eq!(
            send(&mut client, get(OLD, "/", Some(&bearer))?).await?,
            (200, "whole".to_owned())
        );
        // The host dropped the connection after a whole keep-alive answer; the sandbox's stays open. Once the proxy has
        // seen the close (a test-only gate, never racing it), the next request is refused in words, and never judged,
        // audited or sent.
        tokio::time::timeout(support::DEADLINE, f.gateway.keyed_upstream_closes(1)).await?;
        let (status, words) = send(&mut client, get(OLD, "/", Some(&bearer))?).await?;
        assert_eq!(status, 502, "{words}");
        assert!(words.contains("closed this connection"), "{words}");
        assert_eq!(
            hosts.old.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the host read one request"
        );
        assert_eq!(uses(f, "ok").await?, 1, "one use, the one sent");
        Ok(())
    })
    .await
}
