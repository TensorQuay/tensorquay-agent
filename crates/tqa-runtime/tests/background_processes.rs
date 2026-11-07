//! TOOL-U-008: background processes stop when the command that started them returns (ADR-0016 amendment). A real server,
//! a child that left the session with `setsid` and a double-forked child, all ignoring SIGTERM and SIGHUP, are gone by
//! the time the shell call answers, checked with `ps` over the whole process tree. The runtime's real child-subreaper
//! cleanup runs here, so this file holds one test: the subreaper belongs to the whole test process.
#[path = "support/fixture.rs"]
mod support;
use std::{path::Path, sync::Arc};
use support::{Fixture, TestResult, answer, stop, user};
use tqa_testkit::{Script, Scripted};
use tqa_types::{Event, ServerFrame};

/// Starts the three escapes, records their pids, waits until each is running and reads one answer from the server.
const START: &str = r#"python3 -c 'import http.server as h, os, signal
signal.signal(signal.SIGTERM, signal.SIG_IGN); signal.signal(signal.SIGHUP, signal.SIG_IGN)
s = h.HTTPServer(("localhost", 0), h.SimpleHTTPRequestHandler)
open("port.part", "w").write(str(s.server_port)); os.rename("port.part", "port"); s.serve_forever()' >/dev/null 2>&1 &
echo $! > server.pid
setsid sh -c 'trap "" HUP TERM; sleep 1000 & echo $! > session-child.pid; wait' >/dev/null 2>&1 &
echo $! > session-leader.pid
sh -c 'setsid sh -c "trap \"\" HUP TERM; echo \$\$ > double-fork.pid; exec sleep 1000" >/dev/null 2>&1 &'
until [ -s port ] && [ -s session-child.pid ] && [ -s double-fork.pid ]; do sleep 0.05; done
python3 -c 'import urllib.request as u; print("served", u.urlopen("http://localhost:" + open("port").read() + "/").status)'"#;

/// Runs after the first call returned and before the run ends: lists the recorded pids and every direct child of the
/// runtime (where an escaped process lands under its child subreaper). Only this shell itself may be listed; the list
/// stays small because a long one is cut to the tool result cap.
const LOOK: &str = "echo \"self $$\"; ps -o pid=,ppid=,pgid=,sid=,args= --ppid \"$PPID\" \\
    -p \"$(cat server.pid session-leader.pid session-child.pid double-fork.pid | paste -sd, -)\"";

fn shell(command: &str) -> Script {
    Script::new(vec![Scripted::ToolCall {
        name: "shell".into(),
        arguments_json: serde_json::json!({ "command": command }).to_string(),
        split_into: 1,
    }])
}

fn results(f: &Fixture) -> Vec<String> {
    f.gateway
        .stored()
        .iter()
        .filter_map(|frame| match frame {
            ServerFrame::Event {
                event: Event::ToolResult { content, .. },
                ..
            } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

fn recorded(workspace: &Path) -> Result<Vec<u32>, Box<dyn std::error::Error>> {
    let mut pids = Vec::new();
    for name in ["server", "session-leader", "session-child", "double-fork"] {
        pids.push(
            std::fs::read_to_string(workspace.join(format!("{name}.pid")))?
                .trim()
                .parse()?,
        );
    }
    Ok(pids)
}

/// `(pid, ppid)` and the whole line of every process `ps` lists.
fn listed(ps: &str) -> Vec<((u32, u32), &str)> {
    ps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let parent = fields.next()?.parse().ok()?;
            Some(((pid, parent), line))
        })
        .collect()
}

/// Every process below `root`, following parents through the whole tree.
fn descendants(ps: &str, root: u32) -> Vec<String> {
    let processes = listed(ps);
    let mut tree = vec![root];
    let mut found = Vec::new();
    while let Some(parent) = tree.pop() {
        for ((pid, ppid), line) in &processes {
            if *ppid == parent {
                tree.push(*pid);
                found.push((*line).to_owned());
            }
        }
    }
    found
}

/// Lists every process with `ps` from the test: none runs below this process (but that `ps`) or has a recorded pid.
fn nothing_left_below(pids: &[u32]) -> TestResult {
    let looking = std::process::Command::new("ps")
        .args(["-eo", "pid=,ppid=,pgid=,sid=,args="])
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let own = looking.id();
    let output = looking.wait_with_output()?;
    assert!(output.status.success(), "ps: {:?}", output.status);
    let ps = String::from_utf8(output.stdout)?;
    let all = listed(&ps);
    assert!(
        all.iter().any(|((pid, _), _)| *pid == std::process::id()),
        "ps listed this process: {ps}"
    );
    // `ps` lists itself, a child of this process.
    let left: Vec<String> = descendants(&ps, std::process::id())
        .into_iter()
        .filter(|line| !line.trim_start().starts_with(&format!("{own} ")))
        .collect();
    assert_eq!(left, Vec::<String>::new());
    for ((pid, _), line) in all {
        assert!(!pids.contains(&pid), "escaped the tree: {line}");
    }
    Ok(())
}

// TEST: TOOL-U-008
#[tokio::test]
async fn tool_u_008_a_server_and_setsid_or_double_forked_children_are_gone_when_the_command_returns()
-> TestResult {
    let f = Fixture::in_memory(vec![shell(START), shell(LOOK), answer()]).await?;
    let mut deps = f.deps()?;
    deps.cleanup = Arc::new(tqa_runtime::ProcessReaper::new(
        Arc::new(tqa_llm::SystemClock),
        Arc::new(tqa_llm::TokioSleeper),
        deps.limits,
    )?);
    let runtime = tqa_runtime::Runtime::new(deps)?;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let token = shutdown.clone();
    let task = tokio::spawn(async move { runtime.run(token).await });
    f.gateway.set_bound(true);
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    let results = results(&f);
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results[0].contains("served 200"), "{}", results[0]);
    assert!(
        results[0].contains("[background processes were stopped when the command returned]"),
        "{}",
        results[0]
    );
    // Seen from inside the run, right after the first call answered.
    let pids = recorded(&f.directory.path().join("workspace"))?;
    let own: u32 = results[1]
        .lines()
        .find_map(|line| line.strip_prefix("self "))
        .ok_or("the look printed its own pid")?
        .trim()
        .parse()?;
    let inside = listed(&results[1]);
    assert_eq!(
        inside.iter().map(|((pid, _), _)| *pid).collect::<Vec<_>>(),
        [own],
        "only the looking shell runs after the call: {}",
        results[1]
    );
    // And from the test once the runtime stopped (its cleanup kills any direct child, this test's `ps` too): nothing is
    // left anywhere below this process.
    stop(shutdown, task).await?;
    nothing_left_below(&pids)?;
    f.gateway.shutdown().await?;
    Ok(())
}
