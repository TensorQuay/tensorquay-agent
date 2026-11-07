//! TK-U-020: the Gate 7 demo's two scripted agents (WEB-E2E-012), as `tests/e2e/gate7_scenarios.py` writes them for a
//! channel. Each loads as a standalone scenario, every tool call passes the registry's validation of the four channel
//! tools (as the runtime runs them), and the choreography is the demo's: an agent waits (by type) before it acts, so
//! every seq it names is fixed.
use async_trait::async_trait;
use serde_json::Value;
use std::{io::Write, path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;
use tqa_testkit::standalone::Scenario;
use tqa_tools::{
    ToolRegistry,
    builtin::channel::{
        ChannelClient, ChannelFailure, ChannelPage, ChannelSummary, Claimed, ReadRequest, tools,
    },
};
use tqa_types::{ChannelMessage, ChannelPost};

type TestResult = Result<(), Box<dyn std::error::Error>>;
/// Each turn's single tool call as (name, arguments), or `None` for the closing text.
type Steps = Vec<Option<(String, Value)>>;
const CHANNEL: &str = "01a0e36e-9ad6-73d8-a136-9e932914eab6";

/// Registration needs a client; validation never calls it.
struct Unused;
#[async_trait]
impl ChannelClient for Unused {
    async fn channels(&self, _: &CancellationToken) -> Result<Vec<ChannelSummary>, ChannelFailure> {
        Err(ChannelFailure::new("unused", "unused"))
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

/// The generator's scenario for `agent`, for `CHANNEL`, loaded as the standalone server loads it.
fn render(agent: &str) -> Result<(Value, Scenario), Box<dyn std::error::Error>> {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = tempfile::tempdir()?;
    let status = std::process::Command::new("python3")
        .arg("-B")
        .arg(repository.join("tests/e2e/gate7_scenarios.py"))
        .args(["--channel", CHANNEL, "--out"])
        .arg(out.path())
        .status()?;
    assert!(status.success(), "the generator failed");
    let path = out.path().join(format!("{agent}.json"));
    let text = std::fs::read_to_string(&path)?;
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(text.as_bytes())?;
    Ok((serde_json::from_str(&text)?, Scenario::load(file.path())?))
}

/// Each response's single tool call as (name, arguments), or `None` for the closing text.
fn calls(scenario: &Value) -> Result<Steps, Box<dyn std::error::Error>> {
    let mut steps = Vec::new();
    for response in scenario["responses"].as_array().ok_or("responses")? {
        let tool = response["events"]
            .as_array()
            .ok_or("events")?
            .iter()
            .filter(|event| event["type"] == "tool_call")
            .map(|event| {
                Ok::<_, Box<dyn std::error::Error>>((
                    event["name"].as_str().ok_or("name")?.to_owned(),
                    event["arguments"].clone(),
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        assert!(tool.len() <= 1, "one call per turn: {response}");
        steps.push(tool.into_iter().next());
    }
    Ok(steps)
}

fn registry() -> Result<ToolRegistry, Box<dyn std::error::Error>> {
    let mut registry = ToolRegistry::new();
    for tool in tools(Arc::new(Unused)) {
        registry.register(tool)?;
    }
    Ok(registry)
}

/// (tool, the argument that pins the step) for each turn; `None` is the closing text.
fn shape(steps: &[Option<(String, Value)>]) -> Vec<Option<(String, String)>> {
    steps
        .iter()
        .map(|step| {
            step.as_ref().map(|(name, args)| {
                let key = match name.as_str() {
                    "channel_post" => {
                        format!("{}{}", args["type"].as_str().unwrap_or(""), seq_of(args))
                    }
                    "channel_wait" => format!("{}>{}", args["types"], args["since"]),
                    "task_claim" => format!("#{}", args["task_seq"]),
                    _ => String::new(),
                };
                (name.clone(), key)
            })
        })
        .collect()
}

fn seq_of(args: &Value) -> String {
    ["proposal_seq", "task_seq"]
        .iter()
        .find_map(|field| args[*field].as_u64().map(|seq| format!(" #{seq}")))
        .unwrap_or_default()
}

#[test]
fn tk_u_020_both_agents_render_load_and_validate_against_the_real_tools() -> TestResult {
    let registry = registry()?;
    for name in ["planner", "builder"] {
        let (scenario, _loaded) = render(name)?;
        for (tool, args) in calls(&scenario)?.into_iter().flatten() {
            registry
                .prepare(&tool, &args)
                .map_err(|error| format!("{name}: {tool} {args}: {error:?}"))?;
            assert_eq!(args["channel"], CHANNEL, "{name}: {tool} names the channel");
        }
    }
    Ok(())
}

#[test]
fn tk_u_020_the_choreography_names_fixed_seqs_after_each_wait() -> TestResult {
    let some = |tool: &str, key: &str| Some((tool.to_owned(), key.to_owned()));
    let (planner, _) = render("planner")?;
    assert_eq!(
        shape(&calls(&planner)?),
        [
            some("channel_post", "text"),             // 1
            some("channel_post", "proposal"),         // 2
            some("channel_wait", "[\"decision\"]>2"), // 6: accepted
            some("channel_wait", "[\"decision\"]>6"), // 8: the builder's proposal vetoed
            some("channel_post", "task"),             // 9, two claimants
            some("task_claim", "#9"),                 // 10
            some("channel_post", "report #9"),        // 11
            some("channel_wait", "[\"report\"]>11"),  // 13: the person's report
            some("channel_post", "done"),             // 14
            None,
        ]
    );
    let (builder, _) = render("builder")?;
    assert_eq!(
        shape(&calls(&builder)?),
        [
            some("channel_wait", "[\"proposal\"]>0"), // 2
            some("channel_post", "proposal"),         // 3
            some("channel_post", "vote #2"),          // 4
            some("channel_wait", "[\"decision\"]>4"), // 6
            some("channel_wait", "[\"decision\"]>6"), // 8
            None,
        ]
    );
    Ok(())
}
