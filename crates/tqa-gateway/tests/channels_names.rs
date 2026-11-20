//! A member's name in the channel (ADR-0014's amendment, decided on 20 Nov 2026): an agent member is named when it is
//! added (its task's title by default, its owner choosing), renamed by its owner only, unique in the channel without
//! regard to case; every member sees it, `@` matches it (never a title), and an agent's posts carry it (GW-I-375).
#[path = "support/channel_agent.rs"]
mod channel_agent;
#[path = "support/fixture.rs"]
mod support;
use channel_agent::{Agent, WAKE, channel, post, text};
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use uuid::Uuid;

/// Who the channel lists, as one person sees it: each member's name.
async fn names(
    f: &Fixture,
    cookie: &str,
    id: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let (status, listed) = call(
        f,
        Method::GET,
        &format!("/api/v1/channels/{id}/members"),
        cookie,
        (None, None),
    )
    .await?;
    assert_eq!(status, 200, "{listed}");
    Ok(listed["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .filter(|item| item["kind"] == "conversation")
        .filter_map(|item| item["name"].as_str().map(str::to_owned))
        .collect())
}

/// Adds or changes an agent member, answering the status and body.
async fn put_agent(
    f: &Fixture,
    cookie: &str,
    id: &str,
    agent: &Agent,
    body: Value,
) -> Result<(u16, Value), Box<dyn std::error::Error>> {
    let (status, body) = call(
        f,
        Method::PUT,
        &format!("/api/v1/channels/{id}/members/conversation/{}", agent.id),
        cookie,
        (Some(body), None),
    )
    .await?;
    Ok((status, body))
}

/// Whom a message woke.
async fn woke(f: &Fixture, id: &str, seq: u64) -> Result<Vec<Uuid>, Box<dyn std::error::Error>> {
    Ok(
        sqlx::query_scalar("SELECT wakes FROM channel_messages WHERE channel_id = $1 AND seq = $2")
            .bind(Uuid::parse_str(id)?)
            .bind(i64::try_from(seq)?)
            .fetch_one(&f.pool)
            .await?,
    )
}

// TEST: GW-I-375
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_375_a_member_has_a_name_in_the_channel(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (ana, ana_cookie) = f.admin().await?;
        let (daniel, daniel_cookie) = f.member("daniel@example.invalid").await?;
        // Ana's agent, added by Ana without a name: named by its title, which is hers to show.
        let analyst = Agent::new(f, ana.id, "Pricing analyst").await?;
        let id = channel(f, &ana_cookie, "pricing", &[(&analyst, "member")]).await?;
        let (status, body) = call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/user/{}", daniel.id),
            &ana_cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        assert_eq!(status, 200, "{body}");
        // Daniel's agent, its title his: he names it as he chooses, and nobody else sees the title.
        let checker = Agent::new(f, daniel.id, "Data checker for Q4 (private)").await?;
        let (status, body) = put_agent(
            f,
            &daniel_cookie,
            &id,
            &checker,
            json!({"role": "member", "name": "Checker"}),
        )
        .await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["name"], "Checker");
        for cookie in [&ana_cookie, &daniel_cookie] {
            assert_eq!(names(f, cookie, &id).await?, ["Pricing analyst", "Checker"]);
        }
        let (status, listed) = call(
            f,
            Method::GET,
            &format!("/api/v1/channels/{id}/members"),
            &ana_cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        assert!(
            !listed.to_string().contains("private"),
            "Daniel's title reached Ana: {listed}"
        );
        // Unique without regard to case, 1 to 80 characters, no control characters: refused in words.
        let other = Agent::new(f, daniel.id, "Second").await?;
        let (status, body) = put_agent(
            f,
            &daniel_cookie,
            &id,
            &other,
            json!({"role": "member", "name": "checker"}),
        )
        .await?;
        assert_eq!(
            (status, body["error"]["code"].clone()),
            (409, json!("member_name_taken")),
            "{body}"
        );
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("checker"),
            "{body}"
        );
        for bad in [String::new(), "a\nb".to_owned(), "x".repeat(81)] {
            let (status, body) = put_agent(
                f,
                &daniel_cookie,
                &id,
                &other,
                json!({"role": "member", "name": bad}),
            )
            .await?;
            assert_eq!(status, 400, "{body}");
        }
        // Only the agent's owner renames it; renaming its task changes nothing.
        let (status, _) = put_agent(
            f,
            &ana_cookie,
            &id,
            &checker,
            json!({"role": "member", "name": "Mine now"}),
        )
        .await?;
        assert_eq!(status, 403);
        let (status, body) = put_agent(
            f,
            &daniel_cookie,
            &id,
            &checker,
            json!({"role": "member", "name": "EU checker"}),
        )
        .await?;
        assert_eq!(status, 200, "{body}");
        sqlx::query("UPDATE conversations SET title = 'Renamed task' WHERE id = $1")
            .bind(checker.uuid)
            .execute(&f.pool)
            .await?;
        assert_eq!(
            names(f, &ana_cookie, &id).await?,
            ["Pricing analyst", "EU checker"]
        );
        // "@" matches names, for every member; a title no longer wakes anyone.
        let named = post(
            f,
            &ana_cookie,
            &id,
            text("@EU checker please verify the EU rows"),
            None,
        )
        .await?;
        assert_eq!(woke(f, &id, named).await?, [checker.uuid]);
        let titled = post(
            f,
            &ana_cookie,
            &id,
            text("@Data checker for Q4 (private) are you there"),
            None,
        )
        .await?;
        assert!(woke(f, &id, titled).await?.is_empty());
        // An agent's post carries its name, for people and for the agents that read it. Its wake starts its sandbox.
        let mut checker = checker;
        assert!(
            checker.next(f, WAKE).await?.is_some(),
            "the named member's wake"
        );
        let seq = checker.post(f, &id, text("Verified.")).await?;
        let (status, page) = call(
            f,
            Method::GET,
            &format!("/api/v1/channels/{id}/messages?since={}", seq - 1),
            &ana_cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        assert_eq!(
            page["messages"][0]["author"]["name"], "EU checker",
            "{page}"
        );
        Ok(())
    }))
    .await
}
