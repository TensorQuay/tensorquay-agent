//! GW-I-032 (store): the channel log keeps contiguous seqs under concurrent posts and after a refused one, its
//! members and roles, the parent rule and an ended channel; reads page by `since` and `limit` and filter.
use super::{MemberId, NewChannel, add_member, append, append_body, create, read, role, visible};
use crate::GatewayError;
use sqlx::PgPool;
use tqa_types::{
    Author, AuthorKind, ChannelFilter, ChannelPost, ChannelRole, DecisionRule, MessageBody,
    MessageType,
};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn user(pool: &PgPool, email: &str) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, password_hash, role) VALUES ($1, $2::text::citext, 'x', 'member')")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await?;
    Ok(id)
}

async fn conversation(pool: &PgPool, owner: Uuid) -> Result<Uuid, sqlx::Error> {
    let workspace = Uuid::now_v7();
    sqlx::query("INSERT INTO workspaces (id, owner_id, name) VALUES ($1, $2, 'w')")
        .bind(workspace)
        .bind(owner)
        .execute(pool)
        .await?;
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO conversations (id, owner_id, title, model_route, backend, template, workspace_id) \
         VALUES ($1, $2, 'task', 'local', 'docker', 'workstation', $3)",
    )
    .bind(id)
    .bind(owner)
    .bind(workspace)
    .execute(pool)
    .await?;
    Ok(id)
}

fn person(id: Uuid) -> Author {
    Author {
        kind: AuthorKind::User,
        id: id.to_string(),
        name: Some("alice".into()),
    }
}

fn text(words: &str) -> ChannelPost {
    ChannelPost::new(MessageBody::Text { text: words.into() })
}

/// Appends in a transaction of its own, as a route does.
async fn posted(
    pool: &PgPool,
    channel: Uuid,
    author: &Author,
    post: &ChannelPost,
) -> Result<tqa_types::ChannelMessage, GatewayError> {
    let mut transaction = pool.begin().await?;
    let message = append(&mut transaction, channel, author, post).await?;
    transaction.commit().await?;
    Ok(message)
}

async fn posted_body(
    pool: &PgPool,
    channel: Uuid,
    author: &Author,
    body: (Option<u64>, &MessageBody),
) -> Result<tqa_types::ChannelMessage, GatewayError> {
    let mut transaction = pool.begin().await?;
    let message = append_body(&mut transaction, channel, author, body).await?;
    transaction.commit().await?;
    Ok(message)
}

async fn channel(pool: &PgPool, owner: Uuid) -> Result<Uuid, GatewayError> {
    let new = NewChannel {
        owner_id: owner,
        name: "design",
        purpose: "pick a parser",
        rule: DecisionRule::Majority,
    };
    Ok(create(pool, &new).await?.id)
}

// TEST: GW-I-032
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_032_the_creator_leads_and_members_have_roles(pool: PgPool) -> TestResult {
    let owner = user(&pool, "owner@example.invalid").await?;
    let other = user(&pool, "other@example.invalid").await?;
    let agent = conversation(&pool, owner).await?;
    let id = channel(&pool, owner).await?;
    let mut connection = pool.acquire().await?;
    assert_eq!(
        role(&mut connection, id, MemberId::User(owner)).await?,
        Some(ChannelRole::Lead)
    );
    assert_eq!(
        role(&mut connection, id, MemberId::User(other)).await?,
        None
    );
    add_member(
        &mut connection,
        id,
        MemberId::Conversation(agent),
        (ChannelRole::Member, Some(owner)),
    )
    .await?;
    add_member(
        &mut connection,
        id,
        MemberId::User(other),
        (ChannelRole::Observer, Some(owner)),
    )
    .await?;
    assert_eq!(
        role(&mut connection, id, MemberId::Conversation(agent)).await?,
        Some(ChannelRole::Member)
    );
    // Adding again changes the role; there is one row per member.
    add_member(
        &mut connection,
        id,
        MemberId::User(other),
        (ChannelRole::Member, Some(owner)),
    )
    .await?;
    assert_eq!(
        role(&mut connection, id, MemberId::User(other)).await?,
        Some(ChannelRole::Member)
    );
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM channel_members WHERE channel_id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await?;
    assert_eq!(rows, 3);
    Ok(())
}

// TEST: GW-I-032
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_032_seqs_are_contiguous_under_concurrent_posts_and_after_a_refusal(
    pool: PgPool,
) -> TestResult {
    let owner = user(&pool, "owner@example.invalid").await?;
    let id = channel(&pool, owner).await?;
    let author = person(owner);
    let posts = (0..20).map(|index| {
        let (pool, author) = (pool.clone(), author.clone());
        tokio::spawn(
            async move { posted(&pool, id, &author, &text(&format!("post {index}"))).await },
        )
    });
    let mut seqs = Vec::new();
    for post in posts {
        seqs.push(post.await??.seq);
    }
    seqs.sort_unstable();
    assert_eq!(seqs, (1..=20).collect::<Vec<u64>>());
    // A parent that isn't earlier is refused and takes no seq.
    let mut orphan = text("reply");
    orphan.parent = Some(21);
    let refused = posted(&pool, id, &author, &orphan).await;
    assert!(
        matches!(
            refused,
            Err(GatewayError::Invalid {
                field: "parent",
                ..
            })
        ),
        "{refused:?}"
    );
    let mut reply = text("reply");
    reply.parent = Some(3);
    let stored = posted(&pool, id, &author, &reply).await?;
    assert_eq!((stored.seq, stored.parent), (21, Some(3)));
    assert_eq!(stored.author, author, "the author as given");
    Ok(())
}

// TEST: GW-I-032
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_032_an_ended_or_unknown_channel_takes_no_post(pool: PgPool) -> TestResult {
    let owner = user(&pool, "owner@example.invalid").await?;
    let id = channel(&pool, owner).await?;
    sqlx::query("UPDATE channels SET state = 'ended', ended_at = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await?;
    let ended = posted(&pool, id, &person(owner), &text("late")).await;
    assert!(
        matches!(ended, Err(GatewayError::Conflict("channel_ended"))),
        "{ended:?}"
    );
    let mut connection = pool.acquire().await?;
    let stored = visible(&mut connection, (owner, false), id).await?.channel;
    assert!(stored.ended && stored.ended_at.is_some(), "{stored:?}");
    assert_eq!(
        (stored.name.as_str(), stored.rule, stored.owner_id),
        ("design", DecisionRule::Majority, owner)
    );
    assert!(matches!(
        visible(&mut connection, (owner, false), Uuid::now_v7()).await,
        Err(GatewayError::NotFound)
    ));
    let unknown = posted(&pool, Uuid::now_v7(), &person(owner), &text("where")).await;
    assert!(
        matches!(unknown, Err(GatewayError::NotFound)),
        "{unknown:?}"
    );
    Ok(())
}

// TEST: GW-I-032
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_032_reads_page_by_since_and_limit_and_filter(pool: PgPool) -> TestResult {
    let owner = user(&pool, "owner@example.invalid").await?;
    let agent_id = conversation(&pool, owner).await?;
    let id = channel(&pool, owner).await?;
    let agent = Author {
        kind: AuthorKind::Conversation,
        id: agent_id.to_string(),
        name: Some("Coder".into()),
    };
    let human = person(owner);
    posted(&pool, id, &human, &text("hello")).await?; // 1
    let task = MessageBody::Task {
        title: "parse".into(),
        spec: "the file".into(),
        claims: tqa_types::ClaimLimit::default(),
    };
    posted_body(&pool, id, &human, (None, &task)).await?; // 2
    posted_body(
        &pool,
        id,
        &agent,
        (None, &MessageBody::Claim { task_seq: 2 }),
    )
    .await?; // 3
    let mut reply = text("on it");
    reply.parent = Some(2);
    posted(&pool, id, &agent, &reply).await?; // 4
    let report = MessageBody::Report {
        task_seq: 2,
        status: tqa_types::ReportStatus::Completed,
        summary: "done".into(),
        refs: vec![],
    };
    posted_body(&pool, id, &agent, (None, &report)).await?; // 5
    let mut connection = pool.acquire().await?;
    let everything = ChannelFilter::default();
    let seqs = |page: &super::Page| {
        page.messages
            .iter()
            .map(|message| message.seq)
            .collect::<Vec<_>>()
    };
    let page = read(&mut connection, id, (0, &everything, 2)).await?;
    assert_eq!(
        (seqs(&page), page.next_since, page.more),
        (vec![1, 2], 2, true)
    );
    let page = read(&mut connection, id, (2, &everything, 10)).await?;
    assert_eq!(
        (seqs(&page), page.next_since, page.more),
        (vec![3, 4, 5], 5, false)
    );
    let page = read(&mut connection, id, (5, &everything, 10)).await?;
    assert_eq!(
        (seqs(&page), page.next_since, page.more),
        (vec![], 5, false)
    );
    let shared = &pool;
    let filtered = |filter: ChannelFilter| async move {
        read(&mut *shared.acquire().await?, id, (0, &filter, 200))
            .await
            .map(|page| seqs(&page))
    };
    assert_eq!(
        filtered(ChannelFilter {
            types: vec![MessageType::Claim, MessageType::Report],
            ..ChannelFilter::default()
        })
        .await?,
        [3, 5]
    );
    assert_eq!(
        filtered(ChannelFilter {
            parent: Some(2),
            ..ChannelFilter::default()
        })
        .await?,
        [4]
    );
    assert_eq!(
        filtered(ChannelFilter {
            task_seq: Some(2),
            ..ChannelFilter::default()
        })
        .await?,
        [2, 3, 5]
    );
    assert_eq!(
        filtered(ChannelFilter {
            author_kind: Some(AuthorKind::Conversation),
            ..ChannelFilter::default()
        })
        .await?,
        [3, 4, 5]
    );
    // What is stored reads back as it was written.
    let page = read(&mut connection, id, (0, &everything, 200)).await?;
    assert_eq!(page.messages[2].author, agent);
    assert_eq!(page.messages[1].body, task);
    Ok(())
}

// TEST: GW-U-065
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_u_065_a_33rd_member_is_refused_and_a_role_change_at_the_limit_is_not(
    pool: PgPool,
) -> TestResult {
    let owner = user(&pool, "owner@example.invalid").await?;
    let id = channel(&pool, owner).await?;
    let mut connection = pool.acquire().await?;
    // The creator is the first member; 31 more fill the channel.
    let mut members = Vec::new();
    for number in 1..tqa_types::MAX_CHANNEL_MEMBERS {
        let member = user(&pool, &format!("member{number}@example.invalid")).await?;
        add_member(
            &mut connection,
            id,
            MemberId::User(member),
            (ChannelRole::Member, Some(owner)),
        )
        .await?;
        members.push(member);
    }
    let extra = user(&pool, "extra@example.invalid").await?;
    let refused = add_member(
        &mut connection,
        id,
        MemberId::User(extra),
        (ChannelRole::Member, Some(owner)),
    )
    .await;
    assert!(
        matches!(refused, Err(GatewayError::Conflict("channel_full"))),
        "{refused:?}"
    );
    add_member(
        &mut connection,
        id,
        MemberId::User(members[0]),
        (ChannelRole::Observer, Some(owner)),
    )
    .await?;
    assert_eq!(
        role(&mut connection, id, MemberId::User(members[0])).await?,
        Some(ChannelRole::Observer)
    );
    Ok(())
}

/// Fills `channel` with `count` messages dated `hours_ago`, as if posted then.
async fn filled(
    pool: &PgPool,
    channel: Uuid,
    count: i64,
    hours_ago: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO channel_messages (channel_id, seq, author_kind, author_id, author_name, type, body, created_at) \
         SELECT $1, n, 'gateway', 'gateway', 'gateway', 'text', jsonb_build_object('type', 'text', 'text', 'x'), \
         CURRENT_TIMESTAMP - make_interval(hours => $3::int) FROM generate_series(1, $2::bigint) AS n",
    )
    .bind(channel)
    .bind(count)
    .bind(i32::try_from(hours_ago).unwrap_or(i32::MAX))
    .execute(pool)
    .await?;
    sqlx::query("UPDATE channels SET last_seq = $2 WHERE id = $1")
        .bind(channel)
        .bind(count)
        .execute(pool)
        .await?;
    Ok(())
}

// TEST: GW-U-065
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_u_065_the_5001st_message_of_a_day_is_refused_until_the_window_moves_on(
    pool: PgPool,
) -> TestResult {
    let owner = user(&pool, "owner@example.invalid").await?;
    let id = channel(&pool, owner).await?;
    let day = i64::from(tqa_types::MAX_CHANNEL_MESSAGES_PER_DAY);
    filled(&pool, id, day, 1).await?;
    let refused = posted(&pool, id, &person(owner), &text("one too many")).await;
    let Err(GatewayError::ChannelRate(retry)) = refused else {
        return Err(format!(
            "the 5,001st message of a day must be refused with channel_rate: {refused:?}"
        )
        .into());
    };
    // The oldest message of the window is an hour old, so the next slot opens in about 23 hours.
    assert!((22 * 3600..=23 * 3600 + 60).contains(&retry), "{retry}");
    // The refusal took no seq: the log stays contiguous.
    let last: i64 = sqlx::query_scalar("SELECT last_seq FROM channels WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await?;
    assert_eq!(last, day);
    // The gateway's own decision is not a post: a proposal can always close.
    let decision = MessageBody::Decision {
        proposal_seq: 1,
        outcome: tqa_types::Outcome::Accepted,
        rule: DecisionRule::Majority,
    };
    let gateway = Author {
        kind: AuthorKind::Gateway,
        id: "gateway".into(),
        name: Some("gateway".into()),
    };
    posted_body(&pool, id, &gateway, (None, &decision)).await?;
    // Once the day has passed, posting works again.
    sqlx::query("UPDATE channel_messages SET created_at = CURRENT_TIMESTAMP - INTERVAL '25 hours' WHERE channel_id = $1")
        .bind(id)
        .execute(&pool)
        .await?;
    posted(&pool, id, &person(owner), &text("a new day")).await?;
    Ok(())
}

// TEST: GW-U-065
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_u_065_two_adds_racing_for_the_last_place_leave_32_members(pool: PgPool) -> TestResult {
    let owner = user(&pool, "owner@example.invalid").await?;
    let id = channel(&pool, owner).await?;
    let mut connection = pool.acquire().await?;
    for number in 2..tqa_types::MAX_CHANNEL_MEMBERS {
        let member = user(&pool, &format!("member{number}@example.invalid")).await?;
        add_member(
            &mut connection,
            id,
            MemberId::User(member),
            (ChannelRole::Member, Some(owner)),
        )
        .await?;
    }
    drop(connection);
    let (first, second) = (
        user(&pool, "first@example.invalid").await?,
        user(&pool, "second@example.invalid").await?,
    );
    // The first add takes the 32nd place and holds its transaction open; the second starts meanwhile. Without the
    // channel's lock the second counts 31 members too and both land.
    let mut held = pool.begin().await?;
    add_member(
        &mut held,
        id,
        MemberId::User(first),
        (ChannelRole::Member, Some(owner)),
    )
    .await?;
    let racing = tokio::spawn({
        let pool = pool.clone();
        async move {
            let mut transaction = pool.begin().await?;
            add_member(
                &mut transaction,
                id,
                MemberId::User(second),
                (ChannelRole::Member, Some(owner)),
            )
            .await?;
            transaction.commit().await?;
            Ok::<(), GatewayError>(())
        }
    });
    // Time for an unlocked second add to finish before the first commits. With the lock it waits for the commit
    // whatever the delay, so a slow host can only hide the race, never fail the fix.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    held.commit().await?;
    let second_add = racing.await?;
    assert!(
        matches!(second_add, Err(GatewayError::Conflict("channel_full"))),
        "{second_add:?}"
    );
    let members: i64 =
        sqlx::query_scalar("SELECT count(*) FROM channel_members WHERE channel_id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await?;
    assert_eq!(members, 32);
    Ok(())
}
