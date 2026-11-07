//! GW-I-033 (decisions; ADR-0014 point 9, G7): a proposal under `majority` decided by a clear majority, undecided on a
//! tie once everyone voted, and timed out by the sweep; under `human`, agents advise and the first person decides; a
//! person's veto ends a proposal, an agent's is refused; a late vote is refused; ending a channel closes its open
//! proposals.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A channel with `rule`, led by `cookie`'s user, and `members` more people as voting members; their cookies.
async fn channel(
    f: &Fixture,
    cookie: &str,
    rule: &str,
    members: usize,
) -> Result<(String, Vec<String>)> {
    let (_, created) = call(
        f,
        Method::POST,
        "/api/v1/channels",
        cookie,
        (Some(json!({"name": "decide", "rule": rule})), None),
    )
    .await?;
    let id = created["id"].as_str().ok_or("id")?.to_owned();
    let mut cookies = Vec::new();
    for index in 0..members {
        let (user, member_cookie) = f.member(&format!("v{index}@example.invalid")).await?;
        call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/user/{}", user.id),
            cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        cookies.push(member_cookie);
    }
    Ok((id, cookies))
}

async fn post(f: &Fixture, cookie: &str, id: &str, body: Value) -> Result<(u16, Value)> {
    let key = uuid::Uuid::now_v7().to_string();
    call(
        f,
        Method::POST,
        &format!("/api/v1/channels/{id}/messages"),
        cookie,
        (Some(json!({"body": body})), Some(&key)),
    )
    .await
}

async fn propose(f: &Fixture, cookie: &str, id: &str) -> Result<u64> {
    let (status, posted) = post(
        f,
        cookie,
        id,
        json!({"type": "proposal", "title": "use it", "text": "why"}),
    )
    .await?;
    assert_eq!(status, 201, "{posted}");
    posted["seq"].as_u64().ok_or_else(|| "seq".into())
}

fn vote(proposal: u64, choice: &str) -> Value {
    json!({"type": "vote", "proposal_seq": proposal, "choice": choice})
}

async fn state(f: &Fixture, cookie: &str, id: &str, proposal: u64) -> Result<Value> {
    let (status, body) = call(
        f,
        Method::GET,
        &format!("/api/v1/channels/{id}/proposals/{proposal}"),
        cookie,
        (None, None),
    )
    .await?;
    assert_eq!(status, 200, "{body}");
    Ok(body)
}

/// The decisions in the channel's log, as (`proposal_seq`, outcome, author kind).
async fn decisions(f: &Fixture, cookie: &str, id: &str) -> Result<Vec<(u64, String, String)>> {
    let (_, page) = call(
        f,
        Method::GET,
        &format!("/api/v1/channels/{id}/messages?types=decision"),
        cookie,
        (None, None),
    )
    .await?;
    Ok(page["messages"]
        .as_array()
        .ok_or("messages")?
        .iter()
        .map(|message| {
            (
                message["body"]["proposal_seq"].as_u64().unwrap_or_default(),
                message["body"]["outcome"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                message["author"]["kind"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect())
}

// TEST: GW-I-033
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_033_majority_accepts_rejects_ties_and_times_out(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, lead) = f.admin().await?;
        let (id, voters) = channel(f, &lead, "majority", 3).await?; // four voting members
        // A clear majority: three of four accept.
        let accepted = propose(f, &lead, &id).await?;
        for cookie in [&lead, &voters[0]] {
            assert_eq!(post(f, cookie, &id, vote(accepted, "accept")).await?.0, 201);
        }
        assert_eq!(state(f, &lead, &id, accepted).await?["state"], "open", "two of four");
        assert_eq!(post(f, &voters[1], &id, vote(accepted, "accept")).await?.0, 201);
        let decided = state(f, &lead, &id, accepted).await?;
        assert_eq!((&decided["state"], &decided["decision"]["outcome"], &decided["tally"]), (&json!("decided"), &json!("accepted"), &json!({"accept": 3, "reject": 0})));
        // A tie once all four voted, with one change of mind on the way.
        let tied = propose(f, &lead, &id).await?;
        post(f, &lead, &id, vote(tied, "accept")).await?;
        post(f, &voters[0], &id, vote(tied, "accept")).await?;
        post(f, &voters[0], &id, vote(tied, "reject")).await?;
        post(f, &voters[1], &id, vote(tied, "reject")).await?;
        assert_eq!(state(f, &lead, &id, tied).await?["state"], "open", "three of four");
        post(f, &voters[2], &id, vote(tied, "accept")).await?;
        assert_eq!(state(f, &lead, &id, tied).await?["decision"]["outcome"], "undecided");
        // The window closes with votes missing: the sweep decides.
        let late = propose(f, &lead, &id).await?;
        post(f, &lead, &id, vote(late, "accept")).await?;
        sqlx::query("UPDATE channel_messages SET created_at = created_at - interval '11 minutes' WHERE seq = $1 AND channel_id = $2::uuid")
            .bind(i64::try_from(late)?)
            .bind(&id)
            .execute(&f.pool)
            .await?;
        let (status, body) = post(f, &voters[0], &id, vote(late, "accept")).await?;
        assert_eq!((status, &body["error"]["code"]), (409, &json!("proposal_closed")), "a vote after the window");
        assert_eq!(f.gateway.channel_decisions_now().await?, 1);
        assert_eq!(f.gateway.channel_decisions_now().await?, 0, "decided once");
        assert_eq!(
            decisions(f, &lead, &id).await?,
            [(accepted, "accepted".into(), "gateway".into()), (tied, "undecided".into(), "gateway".into()), (late, "timed_out".into(), "gateway".into())]
        );
        // A vote on a decided proposal, and on something that isn't one.
        let (status, body) = post(f, &lead, &id, vote(accepted, "reject")).await?;
        assert_eq!((status, &body["error"]["code"]), (409, &json!("proposal_closed")));
        let (_, text) = post(f, &lead, &id, json!({"type": "text", "text": "hi"})).await?;
        let (status, body) = post(f, &lead, &id, vote(text["seq"].as_u64().ok_or("seq")?, "accept")).await?;
        assert_eq!((status, &body["error"]["code"]), (409, &json!("not_a_proposal")));
        Ok(())
    })
    .await
}

// TEST: GW-I-033
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_033_under_human_agents_advise_a_person_decides_and_vetoes(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, lead) = f.admin().await?;
        let agent = f.conversation(owner.id).await?;
        let _runtime = f.runtime(&agent).await?;
        let token = f
            .backend
            .token(&agent.id)
            .ok_or("token")?
            .expose_secret()
            .to_owned();
        let (id, _) = channel(f, &lead, "human", 0).await?;
        call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/conversation/{}", agent.id),
            &lead,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        let agent_vote = |proposal: u64, choice: &str| {
            f.client
                .post(f.url(&format!("/runtime/channels/{id}/messages")))
                .bearer_auth(&token)
                .json(&json!({"body": vote(proposal, choice)}))
                .send()
        };
        let first = propose(f, &lead, &id).await?;
        assert_eq!(agent_vote(first, "accept").await?.status().as_u16(), 201);
        assert_eq!(
            state(f, &lead, &id, first).await?["state"],
            "open",
            "an agent advises"
        );
        post(f, &lead, &id, vote(first, "reject")).await?;
        assert_eq!(
            state(f, &lead, &id, first).await?["decision"]["outcome"],
            "rejected"
        );
        // A veto: an agent's is refused; a person's ends the proposal; nothing is counted after it.
        let second = propose(f, &lead, &id).await?;
        let refused = agent_vote(second, "veto").await?;
        let status = refused.status().as_u16();
        let body: Value = refused.json().await?;
        assert_eq!(
            (status, &body["error"]["code"]),
            (403, &json!("veto_not_allowed"))
        );
        assert_eq!(post(f, &lead, &id, vote(second, "veto")).await?.0, 201);
        assert_eq!(
            state(f, &lead, &id, second).await?["decision"]["outcome"],
            "vetoed"
        );
        let late = agent_vote(second, "accept").await?;
        assert_eq!(late.status().as_u16(), 409);
        // Ending the channel closes what is still open.
        let third = propose(f, &lead, &id).await?;
        let (status, _) = call(
            f,
            Method::POST,
            &format!("/api/v1/channels/{id}/end"),
            &lead,
            (None, None),
        )
        .await?;
        assert_eq!(status, 200);
        assert_eq!(
            state(f, &lead, &id, third).await?["decision"]["outcome"],
            "timed_out"
        );
        Ok(())
    })
    .await
}
