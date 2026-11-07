//! GW-I-058 (089): the task list is newest-active first, paged by an opaque keyset cursor.

#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn page(f: &Fixture, cookie: &str, query: &str) -> Result<(u16, Value)> {
    let response = f
        .get(&format!("/api/v1/conversations?{query}"), cookie)
        .await?;
    let status = response.status().as_u16();
    Ok((
        status,
        serde_json::from_str(&response.text().await?).unwrap_or(Value::Null),
    ))
}

fn ids(body: &Value) -> Vec<String> {
    body["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Creates a conversation for `owner` whose last activity is `minute` minutes past a fixed instant.
async fn at(f: &Fixture, owner: uuid::Uuid, minute: i32) -> Result<String> {
    let id = f.conversation(owner).await?.id.to_string();
    sqlx::query("UPDATE conversations SET last_activity_at = TIMESTAMPTZ '2026-01-01 00:00:00.123456Z' + make_interval(mins => $2) WHERE id=$1::uuid")
        .bind(&id)
        .bind(minute)
        .execute(&f.pool)
        .await?;
    Ok(id)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_058_newest_activity_first_and_ties_page_by_id_without_gaps(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let first = at(f, admin.id, 1).await?;
        let tied_older = at(f, admin.id, 3).await?;
        let tied_newer = at(f, admin.id, 3).await?;
        let middle = at(f, admin.id, 2).await?;
        let newest = at(f, admin.id, 5).await?;
        let (status, one) = page(f, &cookie, "limit=2").await?;
        assert_eq!(status, 200, "{one}");
        assert_eq!(ids(&one), vec![newest.clone(), tied_newer.clone()]);
        assert!(one.get("next_after").is_none() && one["next_cursor"].is_string());
        let cursor = one["next_cursor"].as_str().ok_or("cursor")?;
        assert!(cursor.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')), "{cursor}");
        let (_, two) = page(f, &cookie, &format!("limit=2&cursor={cursor}")).await?;
        assert_eq!(ids(&two), vec![tied_older, middle.clone()], "the tie continues across the page boundary");
        let (_, three) = page(f, &cookie, &format!("limit=2&cursor={}", two["next_cursor"].as_str().ok_or("cursor")?)).await?;
        assert_eq!((ids(&three), &three["next_cursor"]), (vec![first], &Value::Null));
        // A task that gains activity moves to the top.
        sqlx::query("UPDATE conversations SET last_activity_at = TIMESTAMPTZ '2026-01-01 01:00:00Z' WHERE id=$1::uuid")
            .bind(&middle)
            .execute(&f.pool)
            .await?;
        let (_, top) = page(f, &cookie, "limit=1").await?;
        assert_eq!(ids(&top), vec![middle]);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_058_scoping_bounds_and_malformed_cursors(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (admin, cookie) = f.admin().await?;
        let (member, member_cookie) = f.member("member@example.invalid").await?;
        let own = at(f, admin.id, 1).await?;
        let theirs = at(f, member.id, 2).await?;
        assert_eq!(
            ids(&page(f, &member_cookie, "").await?.1),
            vec![theirs.clone()]
        );
        assert_eq!(ids(&page(f, &cookie, "").await?.1), vec![own.clone()]);
        assert_eq!(
            ids(&page(f, &cookie, "all=true").await?.1),
            vec![theirs, own]
        );
        assert_eq!(page(f, &member_cookie, "all=true").await?.0, 403);
        for limit in ["0", "1001"] {
            assert_eq!(
                page(f, &cookie, &format!("limit={limit}")).await?.0,
                400,
                "{limit}"
            );
        }
        assert_eq!(
            page(f, &cookie, &format!("after={}", uuid::Uuid::nil()))
                .await?
                .0,
            400,
            "after is replaced"
        );
        let long = "A".repeat(200);
        for cursor in ["", "!!!!", "bm90LWEtY3Vyc29y", "djF8eHx5", long.as_str()] {
            let (status, body) = page(f, &cookie, &format!("cursor={cursor}")).await?;
            assert_eq!(
                (status, &body["error"]["details"]),
                (
                    400,
                    &json!([{"field": "cursor", "reason": "cursor_invalid"}])
                ),
                "{cursor:?}"
            );
        }
        Ok(())
    })
    .await
}
