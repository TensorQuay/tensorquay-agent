//! What the web shows on a workspace card - GW-I-030 addendum (owner, quota, share and task
//! counts, the lease) - and GW-I-049, sharing by exact email without ever listing accounts.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::{ConversationId, Event, ServerFrame};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const GIB: u64 = 1024 * 1024 * 1024;

fn call(f: &Fixture, cookie: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

async fn json_of(response: reqwest::Response) -> Result<(u16, Value)> {
    let status = response.status().as_u16();
    Ok((status, response.json().await.unwrap_or(Value::Null)))
}

async fn workspace(f: &Fixture, cookie: &str, name: &str) -> Result<String> {
    let (status, created) = json_of(
        call(f, cookie, Method::POST, "/api/v1/workspaces")
            .json(&json!({ "name": name }))
            .send()
            .await?,
    )
    .await?;
    assert_eq!(status, 201, "{created}");
    Ok(created["id"].as_str().ok_or("id")?.to_owned())
}

async fn listed(f: &Fixture, cookie: &str, id: &str) -> Result<Value> {
    let (status, page) = json_of(
        call(f, cookie, Method::GET, "/api/v1/workspaces")
            .send()
            .await?,
    )
    .await?;
    assert_eq!(status, 200, "{page}");
    let item = page["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["id"] == id)
        .cloned()
        .ok_or("listed")?;
    // The single view agrees with the list.
    let (_, single) = json_of(
        call(f, cookie, Method::GET, &format!("/api/v1/workspaces/{id}"))
            .send()
            .await?,
    )
    .await?;
    agree(&item, &single)?;
    Ok(item)
}

/// The size a background measurement sets, and when; it may land between the list read and the single read.
const MEASURED: [&str; 2] = ["size_bytes", "size_measured_at"];

/// Whether the single view, read after the list, agrees with it: every field equal, except that the size may come
/// from a later measurement. Within each view the size and its time are set together, and the single view's is never
/// older than the list's.
fn agree(listed: &Value, single: &Value) -> Result<()> {
    let without = |view: &Value| {
        let mut rest = view.clone();
        for field in MEASURED {
            rest.as_object_mut().map(|fields| fields.remove(field));
        }
        rest
    };
    if without(listed) != without(single) {
        return Err(format!("the views disagree: {listed} / {single}").into());
    }
    for view in [listed, single] {
        if view[MEASURED[0]].is_null() != view[MEASURED[1]].is_null() {
            return Err(
                format!("a size without its time, or a time without a size: {view}").into(),
            );
        }
    }
    let at = |view: &Value| -> Result<Option<time::OffsetDateTime>> {
        view[MEASURED[1]]
            .as_str()
            .map(|text| {
                time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
            })
            .transpose()
            .map_err(Into::into)
    };
    match (at(listed)?, at(single)?) {
        (Some(first), Some(then)) if then < first => Err(format!(
            "the single view's size is older than the list's: {listed} / {single}"
        )
        .into()),
        (Some(_), None) => {
            Err(format!("the size disappeared between the reads: {listed} / {single}").into())
        }
        (Some(first), Some(then))
            if then == first && listed[MEASURED[0]] != single[MEASURED[0]] =>
        {
            Err(format!("one measurement, two sizes: {listed} / {single}").into())
        }
        _ => Ok(()),
    }
}

// TEST: GW-I-030
#[test]
fn gw_i_030_a_measurement_between_the_reads_is_not_a_disagreement() {
    let view = |size: Value, at: Value, name: &str| json!({"id": "w", "name": name, "size_bytes": size, "size_measured_at": at});
    let (none, early, late) = (
        view(Value::Null, Value::Null, "a"),
        view(json!(10), json!("2026-09-27T10:00:00Z"), "a"),
        view(json!(20), json!("2026-09-27T10:05:00Z"), "a"),
    );
    // Measured between the reads, or measured again: the later read has the newer size.
    for (listed, single) in [
        (&none, &none),
        (&early, &early),
        (&none, &early),
        (&early, &late),
    ] {
        assert!(agree(listed, single).is_ok(), "{listed} / {single}");
    }
    // Anything else is still a disagreement.
    let renamed = view(json!(10), json!("2026-09-27T10:00:00Z"), "b");
    let resized = view(json!(11), json!("2026-09-27T10:00:00Z"), "a");
    let half = view(json!(10), Value::Null, "a");
    for (listed, single) in [
        (&early, &renamed),
        (&late, &early),
        (&early, &none),
        (&early, &resized),
        (&none, &half),
    ] {
        assert!(agree(listed, single).is_err(), "{listed} / {single}");
    }
}

fn event(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_030_a_card_shows_owner_quota_counts_and_the_lease(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (member, member_cookie) = f.member("member@example.invalid").await?;
        let id = workspace(f, &cookie, "Sales analysis").await?;
        let shared = call(
            f,
            &cookie,
            Method::PUT,
            &format!("/api/v1/workspaces/{id}/shares/{}", member.id),
        )
        .json(&json!({"access":"write"}))
        .send()
        .await?;
        assert_eq!(shared.status(), 200);
        let (status, task) = json_of(
            call(f, &cookie, Method::POST, "/api/v1/conversations")
                .header("idempotency-key", "owner-task")
                .json(&json!({"title":"Owner task","model_route":"local","workspace_id":id}))
                .send()
                .await?,
        )
        .await?;
        assert_eq!(status, 201, "{task}");
        let task_id: ConversationId = task["id"].as_str().ok_or("task")?.parse()?;

        let mine = listed(f, &cookie, &id).await?;
        assert_eq!(
            mine["owner"],
            json!({"id": owner.id, "email": "admin@example.invalid"})
        );
        assert_eq!(mine["quota_bytes"], json!(10 * GIB));
        assert_eq!(mine["share_count"], json!(1));
        assert_eq!(mine["task_count"], json!(1));
        assert_eq!(mine["lease"], Value::Null);
        let theirs = listed(f, &member_cookie, &id).await?;
        assert_eq!(theirs["owner"]["email"], "admin@example.invalid");
        assert_eq!(theirs["share_count"], Value::Null);
        assert_eq!(theirs["task_count"], Value::Null);

        // The task's sandbox takes the lease; a run in progress makes it "running".
        let mut runtime = f.runtime_for(&task_id).await?;
        runtime
            .send(&event(
                1,
                Event::RunStarted {
                    run_id: "r-1".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&task_id, 1).await?;
        let lease = json!({"conversation_id": task_id, "title": "Owner task", "running": true, "pinned": false});
        assert_eq!(listed(f, &cookie, &id).await?["lease"], lease);
        // A share holder sees that it is in use, never the title of a task that is not theirs.
        assert_eq!(
            listed(f, &member_cookie, &id).await?["lease"],
            json!({"conversation_id": task_id, "title": null, "running": true, "pinned": false})
        );
        runtime
            .send(&event(
                2,
                Event::RunCompleted {
                    run_id: "r-1".parse()?,
                },
            ))
            .await?;
        f.until_sequence(&task_id, 2).await?;
        assert_eq!(listed(f, &cookie, &id).await?["lease"]["running"], false);
        Ok(())
    })
    .await
}

async fn share_by_email(f: &Fixture, cookie: &str, id: &str, email: &str) -> Result<(u16, Value)> {
    json_of(
        call(
            f,
            cookie,
            Method::PUT,
            &format!("/api/v1/workspaces/{id}/shares"),
        )
        .json(&json!({"email": email, "access": "read"}))
        .send()
        .await?,
    )
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_049_share_by_exact_email_without_listing_accounts(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_owner, cookie) = f.admin().await?;
        let (member, member_cookie) = f.member("Sam@Example.invalid").await?;
        let (gone, _) = f.member("gone@example.invalid").await?;
        sqlx::query!(
            "UPDATE users SET disabled_at=CURRENT_TIMESTAMP WHERE id=$1",
            gone.id
        )
        .execute(&f.pool)
        .await?;
        let id = workspace(f, &cookie, "Handbook").await?;

        // Exact email, case-insensitive.
        let (status, share) = share_by_email(f, &cookie, &id, "sam@example.invalid").await?;
        assert_eq!(status, 200, "{share}");
        assert_eq!(
            share,
            json!({"user_id": member.id, "email": "Sam@Example.invalid", "access": "read"})
        );
        // Unknown and inactive accounts get the same answer.
        let (unknown_status, unknown) =
            share_by_email(f, &cookie, &id, "nobody@example.invalid").await?;
        let (inactive_status, inactive) =
            share_by_email(f, &cookie, &id, "gone@example.invalid").await?;
        assert_eq!((unknown_status, inactive_status), (404, 404));
        assert_eq!(unknown["error"]["code"], "user_not_found");
        assert_eq!(unknown, inactive);
        // A prefix is not a match.
        assert_eq!(share_by_email(f, &cookie, &id, "sam@").await?.0, 404);
        // Only the owner shares.
        assert_eq!(
            share_by_email(f, &member_cookie, &id, "admin@example.invalid")
                .await?
                .0,
            403
        );
        // Rate limited per caller, like sign-in: 10 attempts a minute. The owner has made 4 so far; the 11th is 429,
        // and another caller is not affected.
        let mut statuses = Vec::new();
        for _ in 0..7 {
            statuses.push(
                share_by_email(f, &cookie, &id, "nobody@example.invalid")
                    .await?
                    .0,
            );
        }
        assert_eq!(statuses, [404, 404, 404, 404, 404, 404, 429]);
        assert_eq!(
            share_by_email(f, &member_cookie, &id, "admin@example.invalid")
                .await?
                .0,
            403
        );
        Ok(())
    })
    .await
}
