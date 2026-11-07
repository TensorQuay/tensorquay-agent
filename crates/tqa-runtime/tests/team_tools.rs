//! RT-I-042: the agent-team tools by the role the gateway gives the run at its start (ADR-0034 slice 3; the runtime
//! asks `GET /runtime/teams/role`): a member's two, a task that may lead offered `start_member` alone, none for `none`
//! or a gateway without the route. Unlike the channel tools they come from the role, not the profile's allow-list: a
//! member whose profile lists no tools still gets its two, so it can always report to its lead.
#[path = "support/fixture.rs"]
mod support;
use serde_json::{Value, json};
use support::*;
use tqa_types::ClientFrame;

const TEAM_TOOLS: [&str; 12] = [
    "send_to_lead",
    "wait_for_lead",
    "start_member",
    "assign",
    "ask_all",
    "decide",
    "ask_person",
    "read_reports",
    "done",
    "stop_member",
    "read_helper",
    "take_files",
];

/// The team tools the run's first model request offers.
async fn offered(f: &Fixture) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    stop(shutdown, task).await?;
    let body: Value = f
        .gateway
        .model_requests()
        .into_iter()
        .next()
        .ok_or("no model request")?
        .body;
    let mut names: Vec<String> = body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
        .filter(|name| TEAM_TOOLS.contains(&name.as_str()))
        .collect();
    names.sort();
    Ok(names)
}

// TEST: RT-I-042
#[tokio::test]
async fn rt_i_042_each_role_gets_its_team_tools() -> TestResult {
    for (role, expected) in [
        (Some("member"), vec!["send_to_lead", "wait_for_lead"]),
        (Some("may_lead"), vec!["start_member"]),
        (
            Some("lead"),
            vec![
                "ask_all",
                "ask_person",
                "assign",
                "decide",
                "done",
                "read_helper",
                "read_reports",
                "start_member",
                "stop_member",
                "take_files",
            ],
        ),
        // A lead whose team ended, its end note unread: the read that delivers it.
        (Some("ended"), vec!["read_reports"]),
        (Some("none"), vec![]),
        // An older gateway, without the route.
        (None, vec![]),
    ] {
        let f = Fixture::in_memory(vec![answer()]).await?;
        f.gateway.set_team_role(role);
        assert_eq!(offered(&f).await?, expected, "{role:?}");
    }
    Ok(())
}

// TEST: RT-I-042
#[tokio::test]
async fn rt_i_042_a_member_whose_profile_lists_no_tools_can_still_report() -> TestResult {
    let f = Fixture::in_memory(vec![answer()]).await?;
    f.gateway.set_team_role(Some("member"));
    let mut session = serde_json::to_value(session()?)?;
    session["profile"] = json!({"name": "silent", "tools": []});
    f.gateway
        .set_session(ClientFrame::parse(&serde_json::to_vec(&session)?)?)?;
    assert_eq!(offered(&f).await?, ["send_to_lead", "wait_for_lead"]);
    Ok(())
}

/// The run's log at INFO and above while `f`'s first run goes, for a check that nothing about agent teams was logged.
async fn logged_run(f: &Fixture) -> Result<(Vec<String>, String), Box<dyn std::error::Error>> {
    let path = f.directory.path().join("runtime.log");
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::sync::Mutex::new(std::fs::File::create(&path)?))
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    let names = offered(f).await?;
    drop(guard);
    Ok((names, std::fs::read_to_string(&path)?))
}

// TEST: RT-I-042
#[tokio::test]
async fn rt_i_042_an_older_gateways_missing_role_route_is_silent() -> TestResult {
    // A gateway without the route answers 404: no team tools, and nothing logged about it (no warning per run).
    let f = Fixture::in_memory(vec![answer()]).await?;
    f.gateway.set_team_role(None);
    let (names, log) = logged_run(&f).await?;
    assert!(names.is_empty());
    assert!(!log.contains("agent-team"), "no team warning: {log}");
    Ok(())
}

// TEST: RT-I-042
#[tokio::test]
async fn rt_i_042_a_role_call_that_times_out_leaves_the_run_without_team_tools() -> TestResult {
    // A gateway that never answers the role call: after the call's 3 s bound the run goes on as `none`, with a
    // warning; it never fails and never waits longer.
    let f = Fixture::in_memory(vec![answer()]).await?;
    f.gateway.hang_team_role();
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    let started = std::time::Instant::now();
    f.gateway.send(user()).await?;
    // Longer than the usual wait: the role call alone takes its 3 s bound here.
    let done = |name: &str| {
        f.gateway.frames().iter().any(|frame| {
            matches!(frame, tqa_types::ServerFrame::Event { event, .. } if support::event_name(event) == name)
        })
    };
    while !done("run.completed")
        && !done("run.failed")
        && started.elapsed() < std::time::Duration::from_secs(30)
    {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let took = started.elapsed();
    stop(shutdown, task).await?;
    assert!(
        done("run.completed") && !done("run.failed"),
        "the run completed, not failed, after {took:?}"
    );
    assert!(
        took < std::time::Duration::from_secs(6),
        "the run went on once the role call's 3 s bound passed, no later: {took:?}"
    );
    let body: Value = f
        .gateway
        .model_requests()
        .into_iter()
        .next()
        .ok_or("no model request")?
        .body;
    let team: Vec<&str> = body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .filter(|name| TEAM_TOOLS.contains(name))
        .collect();
    assert!(team.is_empty(), "no team tools: {team:?}");
    // One role call for the run: the start-up pass after registration, with nothing to run, prepares no tools.
    let asked = f
        .gateway
        .requests()
        .iter()
        .filter(|request| request.path == "/runtime/teams/role")
        .count();
    assert_eq!(asked, 1, "the role is asked once per run start");
    Ok(())
}
