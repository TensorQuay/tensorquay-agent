//! TOOL-U-019: the four channel tools against a fake client: their specs, what each sends, what the model sees, the
//! gateway's codes, and a wait that ends within 200 ms of cancel.
use super::{
    ChannelClient, ChannelFailure, ChannelPage, ChannelSummary, Claimed, ReadRequest, tools,
};
use crate::{PreparedAction, Tool, ToolContext, ToolError, ToolName};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use tqa_types::{Author, AuthorKind, ChannelMessage, ChannelPost, MessageBody, MessageType};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// What the fake saw, one line per call.
#[derive(Default)]
struct Fake {
    seen: Mutex<Vec<String>>,
    failure: Option<(&'static str, &'static str)>,
    hang: bool,
    repeat_claim: bool,
}

fn stored(seq: u64, body: MessageBody) -> ChannelMessage {
    ChannelMessage {
        seq,
        ts: "2026-09-27T19:00:00Z".into(),
        author: Author {
            kind: AuthorKind::Conversation,
            id: "c-1".into(),
            name: Some("Coder".into()),
        },
        parent: None,
        body,
    }
}

impl Fake {
    fn note(&self, line: String) -> Result<(), ChannelFailure> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
        match self.failure {
            Some((code, message)) => Err(ChannelFailure::new(code, message)),
            None => Ok(()),
        }
    }
    fn seen(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl ChannelClient for Fake {
    async fn channels(&self, _: &CancellationToken) -> Result<Vec<ChannelSummary>, ChannelFailure> {
        self.note("channels".into())?;
        Ok(vec![ChannelSummary {
            id: "ch-1".into(),
            name: "Refactor".into(),
            purpose: "Split the parser".into(),
            role: "member".into(),
            rule: "majority".into(),
            state: "open".into(),
        }])
    }
    async fn read(
        &self,
        channel: &str,
        request: &ReadRequest,
        cancel: &CancellationToken,
    ) -> Result<ChannelPage, ChannelFailure> {
        let ReadRequest {
            since,
            filter,
            limit,
            wait,
        } = request;
        let filter = serde_json::to_string(filter).unwrap_or_default();
        self.note(format!(
            "read {channel} since={since} limit={limit} wait={wait:?} filter={filter}"
        ))?;
        if self.hang {
            // A client that never answers and never looks at the cancel: only the tool's own cancel can end the call.
            let _ = cancel;
            return std::future::pending().await;
        }
        Ok(ChannelPage {
            messages: vec![
                stored(
                    since + 1,
                    MessageBody::Text {
                        text: "hello".into(),
                    },
                )
                .into(),
            ],
            next_since: since + 1,
            more: false,
        })
    }
    async fn post(
        &self,
        channel: &str,
        post: &ChannelPost,
        key: &str,
        _: &CancellationToken,
    ) -> Result<ChannelMessage, ChannelFailure> {
        self.note(format!(
            "post {channel} key={key} {}",
            serde_json::to_string(post).unwrap_or_default()
        ))?;
        Ok(stored(16, post.body.clone()))
    }
    async fn claim(
        &self,
        channel: &str,
        task_seq: u64,
        key: &str,
        _: &CancellationToken,
    ) -> Result<Claimed, ChannelFailure> {
        self.note(format!("claim {channel} task={task_seq} key={key}"))?;
        Ok(Claimed {
            message: stored(17, MessageBody::Claim { task_seq }),
            existing: self.repeat_claim,
        })
    }
}

/// A context over a fresh workspace; keep the directory alive as long as the context.
fn context() -> Result<(tempfile::TempDir, ToolContext), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let clock = tqa_testkit::FakeClock::new();
    let cx = ToolContext {
        workspace: crate::WorkspaceRoot::open(directory.path())?,
        cancel: CancellationToken::new(),
        clock: Arc::new(clock.clone()),
        sleeper: Arc::new(tqa_testkit::FakeSleeper::new(clock)),
        env_allow: vec![],
        limits: crate::ToolLimits::RECOMMENDED,
    };
    Ok((directory, cx))
}

fn permission(call: &str) -> Result<tqa_types::PermissionContext, serde_json::Error> {
    serde_json::from_value(
        json!({"conversation_id":"c-1","run_id":"r-1","step":1,"attempt":1,"call_id":call}),
    )
}

fn prepared(tool: &dyn Tool, args: Value) -> Result<PreparedAction, Box<dyn std::error::Error>> {
    Ok(PreparedAction::new(
        tool.spec().name.clone(),
        args,
        permission("call-7")?,
    )?)
}

fn tool(fake: &Arc<Fake>, name: &'static str) -> Result<Arc<dyn Tool>, Box<dyn std::error::Error>> {
    let missing = format!("no {name}");
    let found = tools(fake.clone())
        .into_iter()
        .find(|tool| tool.spec().name == ToolName::builtin(name));
    Ok(found.ok_or(missing)?)
}

#[test]
fn tool_u_019_four_specs_strict_valid_and_reads_parallel_safe() {
    let fake = Arc::new(Fake::default());
    // CORE-E-041: a post never resets a repeated failure's count.
    assert!(
        tools(fake.clone())
            .iter()
            .all(|tool| !tool.spec().changes_workspace)
    );
    let specs: Vec<(String, bool, Value)> = tools(fake)
        .iter()
        .map(|tool| {
            (
                tool.spec().name.as_str().to_owned(),
                tool.spec().parallel_safe,
                tool.spec().parameters.clone(),
            )
        })
        .collect();
    let parallel: Vec<(&str, bool)> = specs
        .iter()
        .map(|(name, parallel, _)| (name.as_str(), *parallel))
        .collect();
    assert_eq!(
        parallel,
        [
            ("channel_post", false),
            ("channel_read", true),
            ("channel_wait", true),
            ("task_claim", false),
        ]
    );
    for (name, _, schema) in &specs {
        assert!(tqa_types::strict_compatible(schema), "{name}: {schema}");
    }
}

#[tokio::test]
async fn tool_u_019_post_and_claim_send_one_key_per_call_and_say_what_happened() -> TestResult {
    let (_workspace, cx) = context()?;
    let fake = Arc::new(Fake::default());
    let post = tool(&fake, "channel_post")?;
    let args = json!({"channel":"ch-1","type":"text","text":"ready"});
    let output = post
        .call_prepared(prepared(post.as_ref(), args)?, &cx)
        .await?;
    assert_eq!(output.text, "posted #16 in channel ch-1");
    let claim = tool(&fake, "task_claim")?;
    let output = claim
        .call_prepared(
            prepared(claim.as_ref(), json!({"channel":"ch-1","task_seq":7}))?,
            &cx,
        )
        .await?;
    assert_eq!(output.text, "claimed task #7 in channel ch-1 (claim #17)");
    let seen = fake.seen();
    let key = "key=r-1:call-7";
    assert!(
        seen[0].starts_with("post ch-1 ") && seen[0].contains(key),
        "{seen:?}"
    );
    assert!(
        seen[0].contains(r#""body":{"type":"text","text":"ready"}"#),
        "{seen:?}"
    );
    assert!(
        seen[1].starts_with("claim ch-1 task=7 ") && seen[1].contains(key),
        "{seen:?}"
    );
    // A claimant who already holds the claim is told so; the call succeeded.
    let fake = Arc::new(Fake {
        repeat_claim: true,
        ..Fake::default()
    });
    let claim = tool(&fake, "task_claim")?;
    let output = claim
        .call_prepared(
            prepared(claim.as_ref(), json!({"channel":"ch-1","task_seq":7}))?,
            &cx,
        )
        .await?;
    assert_eq!(
        output.text,
        "you already hold the claim on task #7 in channel ch-1 (claim #17)"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_019_read_lists_channels_without_one_and_pages_with_one() -> TestResult {
    let (_workspace, cx) = context()?;
    let fake = Arc::new(Fake::default());
    let read = tool(&fake, "channel_read")?;
    let listed = read.call(json!({}), &cx).await?.text;
    assert!(listed.starts_with("your channels (1):\n"), "{listed}");
    assert!(
        listed.contains("ch-1: role member, rule majority, open"),
        "{listed}"
    );
    assert!(
        listed.contains(
            "<untrusted-content source=\"channel\" channel=\"ch-1\" field=\"name_and_purpose\""
        ),
        "{listed}"
    );
    let page = read
        .call(
            json!({"channel":"ch-1","since":4,"types":["task","report"],"limit":10}),
            &cx,
        )
        .await?
        .text;
    assert!(
        page.starts_with("channel ch-1: messages 5-5 (1); next since=5\n"),
        "{page}"
    );
    let wait = tool(&fake, "channel_wait")?;
    wait.call(json!({"channel":"ch-1","since":5}), &cx).await?;
    let seen = fake.seen();
    assert_eq!(seen[0], "channels");
    assert_eq!(
        seen[1],
        r#"read ch-1 since=4 limit=10 wait=None filter={"types":["task","report"]}"#
    );
    assert_eq!(
        seen[2],
        "read ch-1 since=5 limit=50 wait=Some(120s) filter={}"
    );
    let _ = MessageType::Task;
    Ok(())
}

#[tokio::test]
async fn tool_u_019_the_gateways_code_reaches_the_model() -> TestResult {
    let (_workspace, cx) = context()?;
    let fake = Arc::new(Fake {
        failure: Some((
            "not_member",
            "this conversation is not a member of the channel",
        )),
        ..Fake::default()
    });
    let read = tool(&fake, "channel_read")?;
    let error = read
        .call(json!({"channel":"ch-9"}), &cx)
        .await
        .err()
        .ok_or("accepted")?;
    assert_eq!(
        error.model_message(),
        "not_member: this conversation is not a member of the channel"
    );
    Ok(())
}

#[tokio::test]
async fn tool_u_019_a_wait_ends_promptly_on_cancel() -> TestResult {
    // The fake never answers, so a wait that ignored the cancel would never end: a few seconds tells the two apart on
    // any host (200 ms measured a loaded one).
    let fake = Arc::new(Fake {
        hang: true,
        ..Fake::default()
    });
    let wait = tool(&fake, "channel_wait")?;
    let (_workspace, cx) = context()?;
    let cancel = cx.cancel.clone();
    let call = tokio::spawn(async move {
        wait.call(
            json!({"channel":"ch-1","since":0,"timeout_seconds":300}),
            &cx,
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let cancelled = Instant::now();
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(10), call).await??;
    assert!(matches!(result, Err(ToolError::Cancelled)), "{result:?}");
    assert!(
        cancelled.elapsed() < Duration::from_secs(5),
        "{:?}",
        cancelled.elapsed()
    );
    Ok(())
}

#[test]
fn tool_u_019_an_idempotency_key_always_fits_the_gateways_rule() {
    let fits = |key: &str| {
        (1..=128).contains(&key.len())
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b":_-".contains(&byte))
    };
    assert_eq!(super::tools::idempotency_key("r-1", "call-7"), "r-1:call-7");
    for call in ["call.7/x y", &"c".repeat(200), "toolu_01Ab+cd="] {
        let key = super::tools::idempotency_key("r-1", call);
        assert!(fits(&key) && key.starts_with("h:"), "{call}: {key}");
        assert_eq!(key, super::tools::idempotency_key("r-1", call), "stable");
    }
    assert_ne!(
        super::tools::idempotency_key("r-1", "a.b"),
        super::tools::idempotency_key("r-2", "a.b")
    );
}

/// A client whose memberships answer is scripted.
struct Members(Result<usize, (&'static str, &'static str)>);

#[async_trait]
impl ChannelClient for Members {
    async fn channels(&self, _: &CancellationToken) -> Result<Vec<ChannelSummary>, ChannelFailure> {
        match self.0 {
            Ok(count) => Ok((0..count)
                .map(|index| ChannelSummary {
                    id: format!("ch-{index}"),
                    name: "n".into(),
                    purpose: String::new(),
                    role: "member".into(),
                    rule: "human".into(),
                    state: "open".into(),
                })
                .collect()),
            Err((code, message)) => Err(ChannelFailure::new(code, message)),
        }
    }
    async fn read(
        &self,
        _: &str,
        _: &ReadRequest,
        _: &CancellationToken,
    ) -> Result<ChannelPage, ChannelFailure> {
        Err(ChannelFailure::new("unused", "unused"))
    }
    async fn post(
        &self,
        _: &str,
        _: &ChannelPost,
        _: &str,
        _: &CancellationToken,
    ) -> Result<ChannelMessage, ChannelFailure> {
        Err(ChannelFailure::new("unused", "unused"))
    }
    async fn claim(
        &self,
        _: &str,
        _: u64,
        _: &str,
        _: &CancellationToken,
    ) -> Result<Claimed, ChannelFailure> {
        Err(ChannelFailure::new("unused", "unused"))
    }
}

#[tokio::test]
async fn tool_u_019_the_tools_exist_only_for_a_run_whose_conversation_is_a_member() {
    let count = |answer| async move {
        super::tools_for_run(Arc::new(Members(answer)), &CancellationToken::new())
            .await
            .len()
    };
    assert_eq!(count(Ok(1)).await, 4);
    assert_eq!(count(Ok(3)).await, 4);
    assert_eq!(count(Ok(0)).await, 0, "no channel");
    assert_eq!(
        count(Err(("not_found", "no such route"))).await,
        0,
        "an older gateway"
    );
    assert_eq!(
        count(Err(("gateway_unreachable", "down"))).await,
        0,
        "any other failure"
    );
}

#[test]
fn tool_u_019_the_registry_accepts_calls_that_leave_optionals_out() -> TestResult {
    // The runtime validates through the registry, which fills each omitted nullable property with null first.
    let mut registry = crate::ToolRegistry::new();
    for tool in tools(Arc::new(Fake::default())) {
        registry.register(tool)?;
    }
    for (name, args) in [
        (
            "channel_post",
            json!({"channel":"ch-1","type":"text","text":"hi"}),
        ),
        (
            "channel_post",
            json!({"channel":"ch-1","type":"task","title":"T","spec":"S","claims":"all"}),
        ),
        (
            "channel_post",
            json!({"channel":"ch-1","type":"task","title":"T","spec":"S","claims":3}),
        ),
        ("channel_read", json!({})),
        (
            "channel_read",
            json!({"channel":"ch-1","author_kind":"user"}),
        ),
        ("channel_wait", json!({"channel":"ch-1","since":0})),
        ("task_claim", json!({"channel":"ch-1","task_seq":7})),
    ] {
        registry
            .prepare(name, &args)
            .map_err(|error| format!("{name} {args}: {error:?}"))?;
    }
    Ok(())
}

// TEST: GW-U-066
#[test]
fn gw_u_066_the_channel_tools_are_the_one_shared_list_a_profile_may_name() {
    let names: Vec<String> = tools(Arc::new(Fake::default()))
        .iter()
        .map(|tool| tool.spec().name.as_str().to_owned())
        .collect();
    assert_eq!(
        names,
        crate::builtin::CHANNEL_TOOL_NAMES,
        "registered names and the shared list"
    );
    for name in crate::builtin::BUILTIN_TOOL_NAMES
        .iter()
        .chain(crate::builtin::CHANNEL_TOOL_NAMES.iter())
    {
        assert!(crate::builtin::profile_tool(name), "{name}");
    }
    assert!(!crate::builtin::profile_tool("teleport"));
}
