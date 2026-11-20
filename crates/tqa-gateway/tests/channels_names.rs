//! A member's name in the channel (ADR-0014's amendment, decided on 20 Nov 2026): an agent member is named when it is
//! added (its task's title by default, its owner choosing), renamed by its owner only, unique in the channel without
//! regard to case; every member sees it, `@` matches it (never a title), and an agent's posts carry it (GW-I-375).
#[path = "support/channel_agent.rs"]
mod channel_agent;
#[path = "support/fixture.rs"]
mod support;
use channel_agent::{Agent, WAKE, channel, post, runtime_call, text};
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
        assert_eq!(body["named"], true);
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
            // In Ana's own thread, where only a name could wake anyone (a top-level post naming no one goes to the
            // whole channel, C-4).
            Some(named),
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

// TEST: GW-I-378
// A member without a name (added before names, or by someone who isn't its owner) is reached by everything but "@":
// a to-do, a proposal, a reply to its post and a person's post to the channel wake it; "@" with its title names no
// one, so a top-level post with it goes to the whole channel (C-4) and still wakes it, as a post to the channel.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_378_an_unnamed_member_is_woken_by_everything_but_at(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (ana, cookie) = f.admin().await?;
        let mut old = Agent::new(f, ana.id, "Old helper").await?;
        let id = channel(f, &cookie, "pricing", &[(&old, "member")]).await?;
        // As on an upgraded installation: added before names existed.
        sqlx::query("UPDATE channel_members SET name = NULL WHERE conversation_id = $1")
            .bind(old.uuid)
            .execute(&f.pool)
            .await?;
        // The members answer says it has no name, so its owner's page can show its title and offer one.
        let (status, listed) = call(
            f,
            Method::GET,
            &format!("/api/v1/channels/{id}/members"),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        let old_row = listed["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .find(|item| item["id"] == json!(old.id))
            .cloned()
            .ok_or("old")?;
        assert_eq!(
            (old_row["named"].clone(), old_row["title"].clone()),
            (json!(false), json!("Old helper")),
            "{old_row}"
        );
        let mut place = 0;
        let task = post(
            f,
            &cookie,
            &id,
            json!({"type": "task", "title": "Recheck VAT", "spec": "Pages 11 and 12."}),
            None,
        )
        .await?;
        let proposal = post(
            f,
            &cookie,
            &id,
            json!({"type": "proposal", "title": "Ship it", "text": "All green."}),
            None,
        )
        .await?;
        let sentence = old.woken(f, &id, place).await?;
        assert!(
            sentence.contains(&format!("a to-do open for claims (message {task})")),
            "{sentence}"
        );
        assert!(
            sentence.contains(&format!("a proposal for your vote (message {proposal})")),
            "{sentence}"
        );
        old.start(f).await?;
        old.read(f, &id, place).await?;
        let own = old.post(f, &id, text("Looking at VAT now.")).await?;
        old.end(f).await?;
        place = proposal;
        // A reply to its post.
        let reply = post(
            f,
            &cookie,
            &id,
            text("Thanks, pages 11 and 12 only."),
            Some(own),
        )
        .await?;
        let sentence = old.woken(f, &id, place).await?;
        assert!(
            sentence.contains(&format!("a reply to your post (message {reply})")),
            "{sentence}"
        );
        old.start(f).await?;
        place = *old.read(f, &id, place).await?.last().ok_or("read")?;
        old.end(f).await?;
        // "@" with its title names no one: a post to the whole channel, which still reaches it.
        let at = post(
            f,
            &cookie,
            &id,
            text("@Old helper what did you find?"),
            None,
        )
        .await?;
        let sentence = old.woken(f, &id, place).await?;
        assert!(
            sentence.contains(&format!("a post to the channel (message {at})")),
            "{sentence}"
        );
        let wakes: Vec<Uuid> = sqlx::query_scalar(
            "SELECT wakes FROM channel_messages WHERE channel_id = $1::uuid AND seq = $2",
        )
        .bind(&id)
        .bind(i64::try_from(at)?)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(wakes, [old.uuid]);
        Ok(())
    }))
    .await
}

// TEST: GW-I-381
// A claim carries its author's name as a post does (C-12, found by S2 on 20 Nov 2026): an agent that claims a to-do is
// named on the claim in the log and on the to-do board, by its name in the channel.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_381_an_agents_claim_carries_its_name(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (ana, cookie) = f.admin().await?;
        let mut dev = Agent::new(f, ana.id, "Dev").await?;
        let id = channel(f, &cookie, "triage", &[(&dev, "member")]).await?;
        let task = post(
            f,
            &cookie,
            &id,
            json!({"type": "task", "title": "Fix the due-date bug", "spec": "See the report."}),
            None,
        )
        .await?;
        assert!(dev.next(f, WAKE).await?.is_some(), "the to-do's wake");
        let (status, claimed) = runtime_call(
            f,
            Method::POST,
            &format!("/runtime/channels/{id}/tasks/{task}/claim"),
            &dev.token(f)?,
            None,
        )
        .await?;
        assert!(status == 200 || status == 201, "{status} {claimed}");
        let (status, page) = call(
            f,
            Method::GET,
            &format!("/api/v1/channels/{id}/messages?since={task}"),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        let claim = page["messages"]
            .as_array()
            .ok_or("messages")?
            .iter()
            .find(|message| message["body"]["type"] == "claim")
            .cloned()
            .ok_or_else(|| format!("no claim in {page}"))?;
        assert_eq!(claim["author"]["name"], "Dev", "{claim}");
        let (status, board) = call(
            f,
            Method::GET,
            &format!("/api/v1/channels/{id}/tasks"),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        assert!(board.to_string().contains("\"name\":\"Dev\""), "{board}");
        Ok(())
    }))
    .await
}

// TEST: GW-I-378
// The two cases left (21 Nov 2026): a member without a name is reached as a writer too. The decision on the proposal it
// made wakes it ("the decision on your proposal"), and a person's report on the to-do it posted wakes it ("a report on
// your to-do"): both find it by its author id, never by a name.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_378_an_unnamed_writer_gets_its_decision_and_its_report(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (ana, cookie) = f.admin().await?;
        let mut old = Agent::new(f, ana.id, "Old helper").await?;
        let id = channel(f, &cookie, "pricing", &[(&old, "member")]).await?;
        sqlx::query("UPDATE channel_members SET name = NULL WHERE conversation_id = $1")
            .bind(old.uuid)
            .execute(&f.pool)
            .await?;
        // A person's post to the channel wakes it; in that run it proposes, votes for it, and posts a to-do.
        post(f, &cookie, &id, text("Anything to settle this week?"), None).await?;
        old.woken(f, &id, 0).await?;
        old.start(f).await?;
        let mut place = *old.read(f, &id, 0).await?.last().ok_or("read")?;
        let proposal = old
            .post(f, &id, json!({"type": "proposal", "title": "Raise EU Pro to 24", "text": "Churn is 1.8 %."}))
            .await?;
        old.post(f, &id, json!({"type": "vote", "proposal_seq": proposal, "choice": "accept"}))
            .await?;
        let todo = old
            .post(f, &id, json!({"type": "task", "title": "Draft the notice", "spec": "Two lines."}))
            .await?;
        // Its own posts don't move its place: the next wake is told from what it read.
        old.end(f).await?;
        // Ana's vote decides it (two of two): the decision wakes its proposer.
        post(f, &cookie, &id, json!({"type": "vote", "proposal_seq": proposal, "choice": "accept"}), None).await?;
        let decision: i64 = sqlx::query_scalar(
            "SELECT seq FROM channel_messages WHERE channel_id = $1::uuid AND type = 'decision'",
        )
        .bind(&id)
        .fetch_one(&f.pool)
        .await?;
        let sentence = old.woken(f, &id, place).await?;
        assert!(sentence.contains(&format!("the decision on your proposal (message {decision})")), "{sentence}");
        old.start(f).await?;
        place = *old.read(f, &id, place).await?.last().ok_or("read")?;
        old.end(f).await?;
        // Ana takes the to-do and reports on it: the report wakes its writer.
        let (status, claimed) = call(
            f,
            Method::POST,
            &format!("/api/v1/channels/{id}/tasks/{todo}/claim"),
            &cookie,
            (None, Some("claim-1")),
        )
        .await?;
        assert_eq!(status, 201, "{claimed}");
        let report = post(
            f,
            &cookie,
            &id,
            json!({"type": "report", "task_seq": todo, "status": "completed", "summary": "Drafted."}),
            None,
        )
        .await?;
        let sentence = old.woken(f, &id, place).await?;
        assert!(sentence.contains(&format!("a report on your to-do (message {report})")), "{sentence}");
        Ok(())
    }))
    .await
}

// TEST: GW-I-391
// The second review of #153 (21 Nov 2026). A role change of an agent added before names keeps it unnamed: its task's
// title becomes its name only when its owner adds it, never by a later "Make lead" (SEC-044). Two members given the same
// name at the same moment: one gets it, the other the worded refusal, never a 503.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_391_a_role_change_never_names_an_agent_and_a_name_race_is_worded(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (ana, cookie) = f.admin().await?;
        let old = Agent::new(f, ana.id, "Acquisition target shortlist").await?;
        let id = channel(f, &cookie, "deals", &[(&old, "member")]).await?;
        sqlx::query("UPDATE channel_members SET name = NULL WHERE conversation_id = $1")
            .bind(old.uuid)
            .execute(&f.pool)
            .await?;
        // What the page sends on "Make lead": the role alone.
        let (status, body) = put_agent(f, &cookie, &id, &old, json!({"role": "lead"})).await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            (&body["named"], &body["role"]),
            (&json!(false), &json!("lead")),
            "{body}"
        );
        let stored: Option<String> =
            sqlx::query_scalar("SELECT name FROM channel_members WHERE conversation_id = $1")
                .bind(old.uuid)
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(stored, None, "the title became its name");
        // The same name twice at once.
        let (first, second) = (
            Agent::new(f, ana.id, "One").await?,
            Agent::new(f, ana.id, "Two").await?,
        );
        let (a, b) = tokio::join!(
            put_agent(
                f,
                &cookie,
                &id,
                &first,
                json!({"role": "member", "name": "Reviewer"})
            ),
            put_agent(
                f,
                &cookie,
                &id,
                &second,
                json!({"role": "member", "name": "reviewer"})
            ),
        );
        let (a, b) = (a?, b?);
        let mut statuses = [a.0, b.0];
        statuses.sort_unstable();
        assert_eq!(
            statuses,
            [200, 409],
            "one gets it, the other is refused in words: {a:?} {b:?}"
        );
        let refused = if a.0 == 409 { &a.1 } else { &b.1 };
        assert_eq!(refused["error"]["code"], "member_name_taken", "{refused}");
        // And two defaults at once: the second takes a number.
        let (third, fourth) = (
            Agent::new(f, ana.id, "Writer").await?,
            Agent::new(f, ana.id, "Writer").await?,
        );
        let (c, d) = tokio::join!(
            put_agent(f, &cookie, &id, &third, json!({"role": "member"})),
            put_agent(f, &cookie, &id, &fourth, json!({"role": "member"})),
        );
        let (c, d) = (c?, d?);
        assert_eq!((c.0, d.0), (200, 200), "{c:?} {d:?}");
        let mut given = [
            c.1["name"].as_str().unwrap_or("").to_owned(),
            d.1["name"].as_str().unwrap_or("").to_owned(),
        ];
        given.sort();
        assert_eq!(given, ["Writer".to_owned(), "Writer 2".to_owned()]);
        Ok(())
    }))
    .await
}
