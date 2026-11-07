//! Channels through the human API (ADR-0014; the contract in the channels API notes):
//! - GW-I-032: create, members with roles, post as member, observer and stranger, read since with a filter, the author
//!   taken from the session;
//! - GW-I-102: who sees a channel (owner, member, a member conversation's owner, an administrator, a stranger), ending
//!   it and deleting it;
//! - posts are idempotent by key;
//! - GW-I-113: an agent member's run status from the lifecycle projection, and none for a person.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn channel(f: &Fixture, cookie: &str) -> Result<String> {
    let (status, body) = call(
        f,
        Method::POST,
        "/api/v1/channels",
        cookie,
        (
            Some(json!({"name": "design", "purpose": "pick a parser", "rule": "majority"})),
            Some("create-1"),
        ),
    )
    .await?;
    assert_eq!(status, 201, "{body}");
    assert_eq!(
        (
            &body["role"],
            &body["state"],
            &body["rule"],
            &body["last_seq"]
        ),
        (
            &json!("lead"),
            &json!("open"),
            &json!("majority"),
            &json!(0)
        )
    );
    Ok(body["id"].as_str().ok_or("id")?.to_owned())
}

async fn post(f: &Fixture, cookie: &str, id: &str, body: Value, key: &str) -> Result<(u16, Value)> {
    call(
        f,
        Method::POST,
        &format!("/api/v1/channels/{id}/messages"),
        cookie,
        (Some(body), Some(key)),
    )
    .await
}

fn text(words: &str) -> Value {
    json!({"body": {"type": "text", "text": words}})
}

async fn member(
    f: &Fixture,
    cookie: &str,
    id: &str,
    (kind, member): (&str, &str),
    role: &str,
) -> Result<Value> {
    let (status, body) = call(
        f,
        Method::PUT,
        &format!("/api/v1/channels/{id}/members/{kind}/{member}"),
        cookie,
        (Some(json!({"role": role})), None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(body)
}

fn code(body: &Value) -> &Value {
    &body["error"]["code"]
}

// TEST: GW-I-032
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_032_members_post_by_role_and_read_since_a_seq(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (bob, bob_cookie) = f.member("bob@example.invalid").await?;
        let (carol, carol_cookie) = f.member("carol@example.invalid").await?;
        let (_, stranger) = f.member("dave@example.invalid").await?;
        let id = channel(f, &cookie).await?;
        let added = member(f, &cookie, &id, ("user", &bob.id.to_string()), "member").await?;
        assert_eq!((&added["kind"], &added["role"], &added["name"]), (&json!("user"), &json!("member"), &json!("bob@example.invalid")));
        member(f, &cookie, &id, ("user", &carol.id.to_string()), "observer").await?;
        let (status, first) = post(f, &cookie, &id, text("hello"), "p1").await?;
        assert_eq!((status, &first["seq"]), (201, &json!(1)), "{first}");
        assert_eq!(first["author"], json!({"kind": "user", "id": owner.id.to_string(), "name": owner.email}));
        // The author comes from the session, never from the body (SEC-015).
        let forged = json!({"body": {"type": "text", "text": "me"}, "author": {"kind": "user", "id": owner.id.to_string()}});
        let (status, second) = post(f, &bob_cookie, &id, forged, "p2").await?;
        assert_eq!((status, &second["seq"], &second["author"]["id"]), (201, &json!(2), &json!(bob.id.to_string())));
        let (status, body) = post(f, &carol_cookie, &id, text("can I?"), "p3").await?;
        assert_eq!((status, code(&body)), (403, &json!("observer")));
        let (status, body) = post(f, &stranger, &id, text("hi"), "p4").await?;
        assert_eq!((status, code(&body)), (404, &json!("not_found")), "a stranger can't tell the channel exists");
        // A decision is the gateway's, a claim is made by the claim route.
        let decision = json!({"body": {"type": "decision", "proposal_seq": 1, "outcome": "accepted", "rule": "human"}});
        let (status, body) = post(f, &cookie, &id, decision, "p5").await?;
        assert_eq!((status, &body["error"]["details"][0]), (400, &json!({"field": "body.type", "reason": "message_invalid"})));
        // An observer reads; a read pages from `since`, filtered.
        let (status, page) = call(f, Method::GET, &format!("/api/v1/channels/{id}/messages?since=1"), &carol_cookie, (None, None)).await?;
        assert_eq!(status, 200, "{page}");
        assert_eq!((&page["messages"][0]["seq"], &page["next_since"], &page["more"]), (&json!(2), &json!(2), &json!(false)));
        let (_, page) = call(f, Method::GET, &format!("/api/v1/channels/{id}/messages?limit=1"), &cookie, (None, None)).await?;
        assert_eq!((&page["next_since"], &page["more"]), (&json!(1), &json!(true)));
        let (_, page) = call(f, Method::GET, &format!("/api/v1/channels/{id}/messages?types=task,vote"), &cookie, (None, None)).await?;
        assert_eq!(page["messages"], json!([]));
        let (status, body) = call(f, Method::GET, &format!("/api/v1/channels/{id}/messages?wait=5"), &cookie, (None, None)).await?;
        assert_eq!((status, &body["error"]["details"][0]["field"]), (400, &json!("wait")), "people wait on the stream");
        let (status, _) = call(f, Method::GET, &format!("/api/v1/channels/{id}/messages"), &stranger, (None, None)).await?;
        assert_eq!(status, 404);
        let (_, members) = call(f, Method::GET, &format!("/api/v1/channels/{id}/members"), &bob_cookie, (None, None)).await?;
        assert_eq!(members["items"].as_array().map(Vec::len), Some(3));
        Ok(())
    })
    .await
}

// TEST: GW-I-032
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_032_a_post_is_idempotent_by_key(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = channel(f, &cookie).await?;
        let (first, stored) = post(f, &cookie, &id, text("once"), "same").await?;
        let (again, replay) = post(f, &cookie, &id, text("once"), "same").await?;
        assert_eq!((first, again, &stored), (201, 201, &replay));
        let (status, body) = post(f, &cookie, &id, text("other"), "same").await?;
        assert_eq!(
            (status, code(&body)),
            (422, &json!("idempotency_key_reused"))
        );
        let (status, _) = call(
            f,
            Method::POST,
            &format!("/api/v1/channels/{id}/messages"),
            &cookie,
            (Some(text("x")), None),
        )
        .await?;
        assert_eq!(status, 400, "the key is required");
        let (_, channel) = call(
            f,
            Method::GET,
            &format!("/api/v1/channels/{id}"),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(channel["last_seq"], 1);
        Ok(())
    })
    .await
}

// TEST: GW-I-102
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_102_who_sees_a_channel_and_ending_and_deleting_it(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (owner, cookie) = f.member("owner@example.invalid").await?;
        let (bob, bob_cookie) = f.member("bob@example.invalid").await?;
        let (_, stranger) = f.member("dave@example.invalid").await?;
        let id = channel(f, &cookie).await?;
        // Bob's conversation is a member; Bob himself is not.
        let agent = f.conversation(bob.id).await?;
        let added = member(f, &cookie, &id, ("conversation", agent.id.as_str()), "member").await?;
        assert_eq!((&added["kind"], &added["owner_name"]), (&json!("conversation"), &json!("bob@example.invalid")));
        let listed = |cookie: String| async move {
            let (_, list) = call(f, Method::GET, "/api/v1/channels", &cookie, (None, None)).await?;
            Ok::<_, Box<dyn std::error::Error>>(list["items"].as_array().map_or(0, Vec::len))
        };
        assert_eq!(listed(cookie.clone()).await?, 1);
        assert_eq!(listed(bob_cookie.clone()).await?, 1, "the owner of a member conversation sees it");
        assert_eq!(listed(admin.clone()).await?, 1);
        assert_eq!(listed(stranger.clone()).await?, 0);
        let (status, seen) = call(f, Method::GET, &format!("/api/v1/channels/{id}"), &bob_cookie, (None, None)).await?;
        assert_eq!((status, &seen["role"]), (200, &Value::Null));
        // Only the conversation's title is private to its owner.
        let (_, members) = call(f, Method::GET, &format!("/api/v1/channels/{id}/members"), &cookie, (None, None)).await?;
        let listed_agent = members["items"].as_array().ok_or("items")?.iter().find(|item| item["kind"] == "conversation").ok_or("agent")?;
        assert!(listed_agent.get("title").is_none(), "{listed_agent}");
        let (_, members) = call(f, Method::GET, &format!("/api/v1/channels/{id}/members"), &bob_cookie, (None, None)).await?;
        assert!(members.to_string().contains("\"title\""), "{members}");
        // Titles are content (SEC-044): an administrator sees the agent's name and owner, never its title.
        let (_, members) = call(f, Method::GET, &format!("/api/v1/channels/{id}/members"), &admin, (None, None)).await?;
        let seen_by_admin = members["items"].as_array().ok_or("items")?.iter().find(|item| item["kind"] == "conversation").ok_or("agent")?;
        assert!(seen_by_admin.get("title").is_none(), "{seen_by_admin}");
        assert_eq!(seen_by_admin["owner_name"], json!("bob@example.invalid"));
        // Whether the caller may open the agent member's task page: its owner, or an administrator (the private view);
        // nobody else, and a person member carries no such flag.
        let path = format!("/api/v1/channels/{id}/members");
        let can_open = |cookie: String| {
            let path = path.clone();
            async move {
            let (_, members) = call(f, Method::GET, &path, &cookie, (None, None)).await?;
            let items = members["items"].as_array().cloned().unwrap_or_default();
            let agent = items.iter().find(|item| item["kind"] == "conversation").cloned().unwrap_or_default();
            let people: Vec<Value> = items.iter().filter(|item| item["kind"] == "user").map(|item| item["can_open"].clone()).collect();
            Ok::<_, Box<dyn std::error::Error>>((agent["can_open"].clone(), people))
            }
        };
        for (who, expected) in [(&cookie, false), (&bob_cookie, true), (&admin, true)] {
            let (agent, people) = can_open(who.clone()).await?;
            assert_eq!(agent, json!(expected));
            assert!(people.iter().all(Value::is_null), "{people:?}");
        }
        // Seeing is not posting.
        for who in [&bob_cookie, &admin] {
            let (status, body) = post(f, who, &id, text("hi"), "x").await?;
            assert_eq!((status, code(&body)), (403, &json!("not_member")));
        }
        let (status, _) = call(f, Method::GET, &format!("/api/v1/channels/{id}"), &stranger, (None, None)).await?;
        assert_eq!(status, 404);
        // Ending: only the owner or an administrator; then nothing more is posted.
        let (status, _) = call(f, Method::POST, &format!("/api/v1/channels/{id}/end"), &bob_cookie, (None, Some("e0"))).await?;
        assert_eq!(status, 403);
        let (status, ended) = call(f, Method::POST, &format!("/api/v1/channels/{id}/end"), &cookie, (None, Some("e1"))).await?;
        assert_eq!((status, &ended["state"]), (200, &json!("ended")), "{ended}");
        assert!(ended["ended_at"].is_string());
        let (status, body) = post(f, &cookie, &id, text("late"), "late").await?;
        assert_eq!((status, code(&body)), (409, &json!("channel_ended")));
        // Deleting: not by a member conversation's owner; by the owner, audited.
        let (status, _) = call(f, Method::DELETE, &format!("/api/v1/channels/{id}"), &bob_cookie, (None, None)).await?;
        assert_eq!(status, 403);
        let (status, _) = call(f, Method::DELETE, &format!("/api/v1/channels/{id}"), &cookie, (None, None)).await?;
        assert_eq!(status, 204);
        let (status, _) = call(f, Method::GET, &format!("/api/v1/channels/{id}"), &cookie, (None, None)).await?;
        assert_eq!(status, 404);
        let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE area = 'channels' AND item = $1 AND action = 'delete' AND actor_user_id = $2")
            .bind(&id)
            .bind(owner.id)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(audited, 1);
        Ok(())
    })
    .await
}

// TEST: GW-I-102
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_102_the_owner_adds_a_person_by_exact_email(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        f.admin().await?;
        let (_, cookie) = f.member("owner@example.invalid").await?;
        let (bob, bob_cookie) = f.member("bob@example.invalid").await?;
        let id = channel(f, &cookie).await?;
        let members = format!("/api/v1/channels/{id}/members");
        let (status, added) = call(
            f,
            Method::PUT,
            &members,
            &cookie,
            (
                Some(json!({"email": "BOB@example.invalid", "role": "member"})),
                None,
            ),
        )
        .await?;
        assert_eq!(status, 200, "{added}");
        assert_eq!(
            (&added["kind"], &added["id"], &added["name"], &added["role"]),
            (
                &json!("user"),
                &json!(bob.id.to_string()),
                &json!("bob@example.invalid"),
                &json!("member")
            )
        );
        // Again: the role changes, still one member.
        let (_, changed) = call(
            f,
            Method::PUT,
            &members,
            &cookie,
            (
                Some(json!({"email": "bob@example.invalid", "role": "observer"})),
                None,
            ),
        )
        .await?;
        assert_eq!(changed["role"], "observer");
        // An unknown address answers as workspace shares do; a member who isn't the owner may not add; the owner is
        // already the lead.
        let (status, body) = call(
            f,
            Method::PUT,
            &members,
            &cookie,
            (
                Some(json!({"email": "nobody@example.invalid", "role": "member"})),
                None,
            ),
        )
        .await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (404, &json!("user_not_found"))
        );
        let (status, body) = call(
            f,
            Method::PUT,
            &members,
            &bob_cookie,
            (
                Some(json!({"email": "owner@example.invalid", "role": "member"})),
                None,
            ),
        )
        .await?;
        assert_eq!((status, &body["error"]["code"]), (403, &json!("not_owner")));
        let (status, _) = call(
            f,
            Method::PUT,
            &members,
            &cookie,
            (
                Some(json!({"email": "owner@example.invalid", "role": "member"})),
                None,
            ),
        )
        .await?;
        assert_eq!(status, 400);
        let (_, listed) = call(f, Method::GET, &members, &cookie, (None, None)).await?;
        assert_eq!(listed["items"].as_array().map(Vec::len), Some(2));
        Ok(())
    })
    .await
}

/// One agent's lifecycle events and its expected status.
type Case<'a> = (&'a [&'a str], &'a str);

/// A lifecycle event as the event store keeps it: the typed `lifecycle` column the members query reads.
async fn lifecycle(f: &Fixture, conversation: uuid::Uuid, seq: i64, kind: &str) -> TestResult {
    sqlx::query(
        "INSERT INTO events (conversation_id, seq, ts, frame, lifecycle) \
         VALUES ($1, $2, CURRENT_TIMESTAMP, '{}'::jsonb, $3)",
    )
    .bind(conversation)
    .bind(seq)
    .bind(kind)
    .execute(&f.pool)
    .await?;
    Ok(())
}

// TEST: GW-I-113
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_113_an_agent_members_run_status_and_none_for_a_person(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let id = channel(f, &cookie).await?;
        let (bob, _) = f.member("bob@example.invalid").await?;
        member(f, &cookie, &id, ("user", &bob.id.to_string()), "member").await?;
        // Each agent's run as the lifecycle projection sees it.
        let cases: [Case<'_>; 7] = [
            (&[], "idle"),
            (&["run.started"], "running"),
            (&["run.started", "run.resumed"], "running"),
            // A run parked for an approval before ADR-0035 has ended.
            (&["run.started", "run.parked"], "failed"),
            (&["run.started", "run.completed"], "completed"),
            (&["run.started", "run.resumed", "run.failed"], "failed"),
            (&["run.started", "run.cancelled"], "cancelled"),
        ];
        let mut expected = std::collections::BTreeMap::new();
        for (events, status) in cases {
            let conversation = f.conversation(owner.id).await?;
            let uuid = uuid::Uuid::parse_str(conversation.id.as_str())?;
            for (index, kind) in events.iter().enumerate() {
                lifecycle(f, uuid, i64::try_from(index)? + 1, kind).await?;
            }
            member(
                f,
                &cookie,
                &id,
                ("conversation", conversation.id.as_str()),
                "member",
            )
            .await?;
            expected.insert(uuid.to_string(), status);
        }
        let (status, listed) = call(
            f,
            Method::GET,
            &format!("/api/v1/channels/{id}/members"),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200, "{listed}");
        let items = listed["items"].as_array().ok_or("items")?;
        for item in items {
            let key = item["id"].as_str().ok_or("id")?;
            match item["kind"].as_str() {
                Some("conversation") => {
                    assert_eq!(item["run_status"], json!(expected[key]), "the agent {key}");
                }
                // A person has no run: the field is absent, not null.
                _ => assert!(item.get("run_status").is_none(), "{item}"),
            }
        }
        assert_eq!(items.len(), 9, "the owner, Bob and the seven agents");
        Ok(())
    })
    .await
}
