//! ADR-0039 slice 5: the task's screen notices, read by its owner.
//! - GW-I-225: `GET /api/v1/conversations/{id}/screen/notices` answers the newest 100 notices oldest first, each with
//!   its kind, who, when and the event it came after, and a hand-back's length and how it ended; another person's task
//!   is `403 forbidden`, a missing one `404`.
//! - GW-I-225: a Take over a restart cut short is handed back at the next start, before the gateway listens (`ended:
//!   restart`, no length), once; a Take over already handed back gets nothing more.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use uuid::Uuid;

/// A notice of `kind` stored for `conversation` by `user`, `minutes` past a fixed hour.
async fn stored(
    f: &Fixture,
    (conversation, user): (Uuid, Uuid),
    kind: &str,
    minutes: i64,
    after_seq: i64,
) -> TestResult {
    let hand_back = kind == "screen.handed_back";
    sqlx::query(
        "INSERT INTO screen_notices (id,conversation_id,kind,by_user,by_name,at,after_seq,seconds,ended) \
         VALUES ($1,$2,$3,$4,'owner@example.invalid',TIMESTAMPTZ '2026-10-05 12:00:00Z' + make_interval(mins => $5::int),$6,$7,$8)",
    )
    .bind(Uuid::now_v7())
    .bind(conversation)
    .bind(kind)
    .bind(user)
    .bind(i32::try_from(minutes)?)
    .bind(after_seq)
    .bind(hand_back.then_some(95_i64))
    .bind(hand_back.then_some("socket_closed"))
    .execute(&f.pool)
    .await?;
    Ok(())
}

async fn notices(
    f: &Fixture,
    id: &str,
    cookie: &str,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    call(
        f,
        Method::GET,
        &format!("/api/v1/conversations/{id}/screen/notices"),
        cookie,
        (None, None),
    )
    .await
}

// TEST: GW-I-225
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_225_the_owner_reads_the_newest_notices_oldest_first(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        let id = Uuid::parse_str(task.id.as_str())?;
        let (status, body) = notices(f, task.id.as_str(), &cookie).await?;
        assert_eq!((status, &body), (200, &json!({"items": []})));
        // Stored out of order: the answer is by time.
        stored(f, (id, owner.id), "screen.handed_back", 2, 7).await?;
        stored(f, (id, owner.id), "screen.taken_over", 1, 7).await?;
        stored(f, (id, owner.id), "screen.signed_out", 3, 9).await?;
        let (status, body) = notices(f, task.id.as_str(), &cookie).await?;
        assert_eq!(status, 200, "{body}");
        let by = json!({"id": owner.id.to_string(), "name": "owner@example.invalid"});
        assert_eq!(
            body,
            json!({"items": [
                {"kind": "screen.taken_over", "by": by, "at": "2026-10-05T12:01:00Z", "after_seq": 7},
                {"kind": "screen.handed_back", "by": by, "at": "2026-10-05T12:02:00Z", "after_seq": 7,
                    "seconds": 95, "ended": "socket_closed"},
                {"kind": "screen.signed_out", "by": by, "at": "2026-10-05T12:03:00Z", "after_seq": 9},
            ]})
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-225
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_225_only_the_newest_100_are_read(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        let id = Uuid::parse_str(task.id.as_str())?;
        for minute in 0..101 {
            stored(f, (id, owner.id), "screen.signed_out", minute, minute).await?;
        }
        let (status, body) = notices(f, task.id.as_str(), &cookie).await?;
        assert_eq!(status, 200, "{body}");
        let items = body["items"].as_array().ok_or("items")?;
        assert_eq!(items.len(), 100);
        // The oldest is dropped; the rest are oldest first.
        assert_eq!(items[0]["after_seq"], 1);
        assert_eq!(items[99]["after_seq"], 100);
        Ok(())
    })
    .await
}

// TEST: GW-I-225
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_225_another_persons_task_is_forbidden_and_a_missing_one_not_found(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, _) = f.admin().await?;
        let (_, other_cookie) = f.member("other@example.invalid").await?;
        let task = f.conversation(owner.id).await?;
        let (status, body) = notices(f, task.id.as_str(), &other_cookie).await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (403, &json!("forbidden")),
            "{body}"
        );
        let (status, _) = notices(f, &Uuid::now_v7().to_string(), &other_cookie).await?;
        assert_eq!(status, 404);
        Ok(())
    })
    .await
}

/// The kind, how it ended and how long, of `conversation`'s newest notice.
async fn newest(
    f: &Fixture,
    conversation: Uuid,
) -> Result<(String, Option<String>, Option<i64>), sqlx::Error> {
    sqlx::query_as(
        "SELECT kind, ended, seconds FROM screen_notices WHERE conversation_id = $1 ORDER BY at DESC, id DESC LIMIT 1",
    )
    .bind(conversation)
    .fetch_one(&f.pool)
    .await
}

// TEST: GW-I-225
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_225_a_take_over_cut_short_by_a_restart_is_handed_back_at_start(
    pool: PgPool,
) -> TestResult {
    support::run_restarting_with(pool, None, "", async |f| {
        let (owner, _) = f.admin().await?;
        let held = Uuid::parse_str(f.conversation(owner.id).await?.id.as_str())?;
        let ended = Uuid::parse_str(f.conversation(owner.id).await?.id.as_str())?;
        sqlx::query(
            "INSERT INTO screen_notices (id,conversation_id,kind,by_user,by_name,at,after_seq) \
             VALUES ($1,$2,'screen.taken_over',$3,'owner@example.invalid',CURRENT_TIMESTAMP - interval '90 seconds',0)",
        )
        .bind(Uuid::now_v7())
        .bind(held)
        .bind(owner.id)
        .execute(&f.pool)
        .await?;
        stored(f, (ended, owner.id), "screen.taken_over", 1, 0).await?;
        stored(f, (ended, owner.id), "screen.handed_back", 2, 0).await?;
        // Recorded before the restarted gateway listens: there as soon as it answers.
        f.restart().await?;
        assert_eq!(
            newest(f, held).await?,
            ("screen.handed_back".to_owned(), Some("restart".to_owned()), None)
        );
        // Already handed back: nothing more; and another start adds nothing either.
        assert_eq!(newest(f, ended).await?.0, "screen.handed_back");
        f.restart().await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM screen_notices WHERE kind = 'screen.handed_back'")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(count, 2);
        Ok(())
    })
    .await
}
