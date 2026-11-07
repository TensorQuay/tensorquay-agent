//! TOOL-I-015: the channel tools end to end: the runtime's client and the four tools against the gateway's runtime
//! routes (ADR-0014 point 6; G4). A member run lists its channels, posts once per call key and reads what it posted as
//! marked channel content; a run whose conversation isn't a member gets the gateway's `not_member`. A wait returns as
//! soon as someone posts, says so when it runs out, and ends promptly when cancelled (G5). A task posted by the run is
//! claimed once by it (a repeat is its existing claim), refused `already_claimed` to another run when full, and a
//! message that isn't a task is `not_a_task` (G6).
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use support::{Fixture, TestResult, call};
use tqa_runtime::testing::GatewayChannels;
use tqa_tools::{PreparedAction, Tool, ToolContext, builtin::channel::tools};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The four tools of `conversation`'s run, over the real routes.
fn run_tools(f: &Fixture, conversation: &tqa_types::ConversationId) -> Result<Vec<Arc<dyn Tool>>> {
    let token = f.backend.token(conversation).ok_or("runtime token")?;
    let origin = url::Url::parse(&f.url(""))?;
    Ok(tools(Arc::new(GatewayChannels::new(&origin, &token)?)))
}

fn tool(tools: &[Arc<dyn Tool>], name: &'static str) -> Result<Arc<dyn Tool>> {
    let missing = format!("no {name}");
    Ok(tools
        .iter()
        .find(|tool| tool.spec().name.as_str() == name)
        .cloned()
        .ok_or(missing)?)
}

fn context() -> Result<(tempfile::TempDir, ToolContext)> {
    let directory = tempfile::tempdir()?;
    let clock = tqa_testkit::FakeClock::new();
    let cx = ToolContext {
        workspace: tqa_tools::WorkspaceRoot::open(directory.path())?,
        cancel: tokio_util::sync::CancellationToken::new(),
        clock: Arc::new(clock.clone()),
        sleeper: Arc::new(tqa_testkit::FakeSleeper::new(clock)),
        env_allow: vec![],
        limits: tqa_tools::ToolLimits::RECOMMENDED,
    };
    Ok((directory, cx))
}

fn prepared(tool: &dyn Tool, args: serde_json::Value, call_id: &str) -> Result<PreparedAction> {
    let context = serde_json::from_value(
        json!({"conversation_id":"c-1","run_id":"r-1","step":1,"attempt":1,"call_id":call_id}),
    )?;
    Ok(PreparedAction::new(
        tool.spec().name.clone(),
        args,
        context,
    )?)
}

// TEST: TOOL-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn tool_i_015_a_member_run_lists_posts_once_per_call_and_reads_marked_content(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let member = f.conversation(owner.id).await?;
        let outsider = f.conversation(owner.id).await?;
        let _member_runtime = f.runtime(&member).await?;
        let _outsider_runtime = f.runtime(&outsider).await?;
        let (status, created) = call(
            f,
            Method::POST,
            "/api/v1/channels",
            &cookie,
            (
                Some(json!({"name": "build", "purpose": "ship it", "rule": "majority"})),
                None,
            ),
        )
        .await?;
        assert_eq!(status, 201, "{created}");
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        let (status, _) = call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/conversation/{}", member.id),
            &cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        assert_eq!(status, 200);
        let (_workspace, cx) = context()?;
        let tools = run_tools(f, &member.id)?;
        let (read, post) = (tool(&tools, "channel_read")?, tool(&tools, "channel_post")?);

        // Without a channel, read lists the memberships, the words marked as channel content.
        let listed = read.call(json!({}), &cx).await?.text;
        assert!(listed.starts_with("your channels (1):\n"), "{listed}");
        assert!(
            listed.contains(&format!("{id}: role member, rule majority, open")),
            "{listed}"
        );
        assert!(listed.contains("field=\"name_and_purpose\""), "{listed}");

        // A post, then the same call again (a retry): one message.
        let args = json!({"channel": id, "type": "text", "text": "tests pass on my branch"});
        let first = post
            .call_prepared(prepared(post.as_ref(), args.clone(), "call-1")?, &cx)
            .await?
            .text;
        let again = post
            .call_prepared(prepared(post.as_ref(), args, "call-1")?, &cx)
            .await?
            .text;
        assert_eq!(first, format!("posted #1 in channel {id}"));
        assert_eq!(
            again, first,
            "a retry with the same call key returns the first answer"
        );

        // The read shows it as one marked block with the gateway's record: seq 1, text, by this conversation.
        let page = read
            .call(json!({"channel": id, "since": 0}), &cx)
            .await?
            .text;
        assert!(
            page.starts_with(&format!("channel {id}: messages 1-1 (1); next since=1\n")),
            "{page}"
        );
        assert!(
            page.contains(" seq=\"1\" type=\"text\" author_kind=\"conversation\""),
            "{page}"
        );
        assert!(page.contains("tests pass on my branch"), "{page}");

        // Another conversation's run: the gateway's not_member reaches the model.
        let outsider_tools = run_tools(f, &outsider.id)?;
        let refused = tool(&outsider_tools, "channel_read")?
            .call(json!({"channel": id}), &cx)
            .await
            .err()
            .ok_or("an outsider read the channel")?;
        assert!(
            refused.model_message().starts_with("not_member: "),
            "{}",
            refused.model_message()
        );
        Ok(())
    })
    .await
}

// TEST: TOOL-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn tool_i_015_a_wait_returns_on_a_post_runs_out_or_ends_on_cancel(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let member = f.conversation(owner.id).await?;
        let _runtime = f.runtime(&member).await?;
        let (_, created) = call(
            f,
            Method::POST,
            "/api/v1/channels",
            &cookie,
            (Some(json!({"name": "build", "purpose": "ship it", "rule": "human"})), None),
        )
        .await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        let (status, _) = call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/conversation/{}", member.id),
            &cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        assert_eq!(status, 200);
        let tools = run_tools(f, &member.id)?;
        let wait = tool(&tools, "channel_wait")?;

        // A person posts while the run waits: the wait returns with that message, marked as a user's.
        let (_workspace, cx) = context()?;
        let waiting = {
            let (wait, id) = (wait.clone(), id.clone());
            let (workspace, cx) = context()?;
            tokio::spawn(async move {
                let _held = workspace;
                wait.call(json!({"channel": id, "since": 0, "timeout_seconds": 60}), &cx).await
            })
        };
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let started = std::time::Instant::now();
        let (status, posted) = call(
            f,
            Method::POST,
            &format!("/api/v1/channels/{id}/messages"),
            &cookie,
            (Some(json!({"body": {"type": "text", "text": "please rebase"}})), Some("human-post-1")),
        )
        .await?;
        assert_eq!(status, 201, "{posted}");
        let page = tokio::time::timeout(std::time::Duration::from_secs(10), waiting).await???.text;
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
        assert!(page.contains(" seq=\"1\" type=\"text\" author_kind=\"user\""), "{page}");
        assert!(page.contains("please rebase"), "{page}");

        // Nothing new within a second: the wait says so and keeps the cursor.
        let quiet = wait
            .call(json!({"channel": id, "since": 1, "timeout_seconds": 1}), &cx)
            .await?
            .text;
        assert_eq!(
            quiet,
            format!("channel {id}: no new messages; next since=1\nthe wait ended without a match; call again to keep waiting\n")
        );

        // Cancelled during a 300 s wait: the tool ends within a few seconds, which tells an observed cancel from an
        // ignored one on any host (200 ms measured a loaded one).
        let (_keep, long_cx) = context()?;
        let cancel = long_cx.cancel.clone();
        let long = {
            let (wait, id) = (wait.clone(), id.clone());
            tokio::spawn(async move {
                wait.call(json!({"channel": id, "since": 1, "timeout_seconds": 300}), &long_cx).await
            })
        };
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let cancelled_at = std::time::Instant::now();
        cancel.cancel();
        let result = tokio::time::timeout(std::time::Duration::from_secs(10), long).await??;
        assert!(cancelled_at.elapsed() < std::time::Duration::from_secs(5), "{:?}", cancelled_at.elapsed());
        assert!(matches!(result, Err(tqa_tools::ToolError::Cancelled)), "{result:?}");
        Ok(())
    })
    .await
}

// TEST: TOOL-I-015
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn tool_i_015_a_task_is_claimed_once_up_to_its_limit(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let first = f.conversation(owner.id).await?;
        let second = f.conversation(owner.id).await?;
        let _first_runtime = f.runtime(&first).await?;
        let _second_runtime = f.runtime(&second).await?;
        let (_, created) = call(
            f,
            Method::POST,
            "/api/v1/channels",
            &cookie,
            (Some(json!({"name": "build", "purpose": "ship it", "rule": "majority"})), None),
        )
        .await?;
        let id = created["id"].as_str().ok_or("id")?.to_owned();
        for conversation in [&first, &second] {
            let (status, _) = call(
                f,
                Method::PUT,
                &format!("/api/v1/channels/{id}/members/conversation/{}", conversation.id),
                &cookie,
                (Some(json!({"role": "member"})), None),
            )
            .await?;
            assert_eq!(status, 200);
        }
        let (_workspace, cx) = context()?;
        let (mine, theirs) = (run_tools(f, &first.id)?, run_tools(f, &second.id)?);
        let post = tool(&mine, "channel_post")?;
        let task = json!({"channel": id, "type": "task", "title": "Fix the parser", "spec": "Make the tests pass"});
        assert_eq!(
            post.call_prepared(prepared(post.as_ref(), task, "call-task")?, &cx).await?.text,
            format!("posted #1 in channel {id}")
        );
        let note = json!({"channel": id, "type": "text", "text": "on it"});
        post.call_prepared(prepared(post.as_ref(), note, "call-note")?, &cx).await?;

        // Claimed once; the claimant's repeat is its existing claim.
        let claim = tool(&mine, "task_claim")?;
        let args = json!({"channel": id, "task_seq": 1});
        let claimed = claim.call_prepared(prepared(claim.as_ref(), args.clone(), "call-claim")?, &cx).await?.text;
        assert!(claimed.starts_with(&format!("claimed task #1 in channel {id} (claim #")), "{claimed}");
        let repeat = claim.call_prepared(prepared(claim.as_ref(), args.clone(), "call-claim-2")?, &cx).await?.text;
        assert!(repeat.starts_with(&format!("you already hold the claim on task #1 in channel {id}")), "{repeat}");

        // Full (one claimant by default): another run is refused; a message that isn't a task can't be claimed.
        let refused = tool(&theirs, "task_claim")?
            .call(args, &cx)
            .await
            .err()
            .ok_or("a second claimant got a one-claimant task")?;
        assert!(refused.model_message().starts_with("already_claimed: "), "{}", refused.model_message());
        let not_task = claim
            .call(json!({"channel": id, "task_seq": 2}), &cx)
            .await
            .err()
            .ok_or("claimed a text message")?;
        assert!(not_task.model_message().starts_with("not_a_task: "), "{}", not_task.model_message());
        Ok(())
    })
    .await
}
