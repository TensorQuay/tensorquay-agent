//! RT-I-001/005/009/034 and Linux process hygiene against the real hardened runtime image.
#[path = "support/container.rs"]
mod container;
#[path = "support/demo_long.rs"]
mod demo_long;
#[path = "support/offline.rs"]
mod offline;
#[path = "support/fixture.rs"]
mod support;
use container::DockerFixture;
use support::{TestResult, answer, user};
use tqa_testkit::{Script, Scripted};
use tqa_types::{Event, ServerFrame};

fn shell(command: &str) -> Script {
    Script::new(vec![Scripted::ToolCall {
        name: "shell".into(),
        arguments_json: serde_json::json!({"command":command}).to_string(),
        split_into: 1,
    }])
}

#[tokio::test]
#[ignore = "requires Docker and the built tqa-runtime:dev image"]
async fn rt_i_001_linux_escape_cleanup_uid_tini_and_no_listening_socket() -> TestResult {
    let f = DockerFixture::start(vec![
        shell("setsid sleep 1000 >/dev/null 2>&1 & echo escaped=$!"),
        answer(),
    ])
    .await?;
    let result = async {
        f.gateway.send(user()).await?;
        f.wait_event("run.completed").await?;
        let result = f
            .gateway
            .stored()
            .iter()
            .find_map(|frame| match frame {
                ServerFrame::Event {
                    event:
                        Event::ToolResult {
                            content, status, ..
                        },
                    ..
                } => Some((content.clone(), *status)),
                _ => None,
            })
            .ok_or("no shell result")?;
        assert_eq!(
            result.1,
            tqa_types::ToolStatus::Ok,
            "shell cleanup failed: {}",
            result.0
        );
        assert!(result.0.contains("escaped="));
        assert_eq!(f.exec(&["id", "-u"]).await?.trim(), "1000");
        assert_eq!(f.exec(&["cat", "/proc/1/comm"]).await?.trim(), "tini");
        f.exec(&["test", "!", "-e", "/usr/local/bin/workspace-clone-probe"])
            .await?;
        let inspected = f
            .exec(&[
                "python3",
                "-c",
                include_str!("support/inspect_processes.py"),
            ])
            .await?;
        assert_eq!(
            inspected.trim(),
            "no children, zombies or listening sockets"
        );
        Ok(())
    }
    .await;
    f.remove().await?;
    result
}

#[tokio::test]
#[ignore = "requires Docker and the built tqa-runtime:dev image"]
async fn rt_i_009_sigterm_during_shell_exits_zero_and_restart_interrupts_the_call() -> TestResult {
    let f = DockerFixture::start(vec![
        shell("sleep 1000 & echo $! > sleeper.pid; wait"),
        answer(),
    ])
    .await?;
    let result=async {
        f.gateway.send(user()).await?;f.wait_event("tool.start").await?;
        f.wait_file("sleeper.pid").await?;
        f.command(&["stop","--time","10"]).await?;
        assert_eq!(f.command(&["inspect","--format","{{.State.ExitCode}}"] ).await?.trim(),"0");
        assert!(!f.gateway.stored().iter().any(terminal));
        f.command(&["start"]).await?;f.gateway.wait_for_registrations(2).await?;f.wait_event("run.completed").await?;
        assert!(f.gateway.stored().iter().any(|frame|matches!(frame,ServerFrame::Event {event:Event::ToolResult {status:tqa_types::ToolStatus::Interrupted,content,..},..} if content.contains("the sandbox stopped"))));
        Ok(())
    }.await;
    f.remove().await?;
    result
}

#[tokio::test]
#[ignore = "requires Docker and the built tqa-runtime:dev image"]
async fn rt_i_005_kill_nine_restarts_on_the_same_log_without_sequence_loss() -> TestResult {
    let f = DockerFixture::start(vec![
        Script::new(vec![
            Scripted::Text("partial".into()),
            Scripted::Delay(std::time::Duration::from_secs(20)),
        ]),
        answer(),
    ])
    .await?;
    let result = async {
        f.gateway.send(user()).await?;
        f.wait_event("text.delta").await?;
        let prefix = f.gateway.stored();
        f.command(&["kill", "--signal", "KILL"]).await?;
        f.command(&["start"]).await?;
        f.gateway.wait_for_registrations(2).await?;
        f.wait_event("run.completed").await?;
        let frames = f.gateway.stored();
        assert_eq!(&frames[..prefix.len()], prefix);
        for (index, frame) in frames.iter().enumerate() {
            assert_eq!(serde_json::to_value(frame)?["seq"], index + 1);
        }
        assert_eq!(
            frames
                .iter()
                .filter(|frame| matches!(
                    frame,
                    ServerFrame::Event {
                        event: Event::RunResumed { .. },
                        ..
                    }
                ))
                .count(),
            1
        );
        Ok(())
    }
    .await;
    f.remove().await?;
    result
}
fn terminal(frame: &ServerFrame) -> bool {
    matches!(
        frame,
        ServerFrame::Event {
            event: Event::RunCompleted { .. }
                | Event::RunFailed { .. }
                | Event::RunCancelled { .. },
            ..
        }
    )
}

#[tokio::test]
#[ignore = "requires Docker and the built runtime image"]
async fn web_e2e_018_long_scenario_finishes_after_one_failed_shell_and_a_fix() -> TestResult {
    let f = DockerFixture::start(demo_long::scripts()?).await?;
    let result = async {
        f.gateway.send(user()).await?;
        f.wait_event("run.completed").await?;
        let frames = f.gateway.stored();
        let results: Vec<_> = frames
            .iter()
            .filter_map(|frame| match frame {
                ServerFrame::Event {
                    event: Event::ToolResult { status, .. },
                    ..
                } => Some(status),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 12);
        let failures: Vec<_> = frames
            .iter()
            .filter_map(|frame| match frame {
                ServerFrame::Event {
                    event:
                        Event::ToolResult {
                            status: tqa_types::ToolStatus::Error,
                            content,
                            ..
                        },
                    ..
                } => Some(content),
                _ => None,
            })
            .collect();
        assert_eq!(failures.len(), 1, "{failures:#?}");
        let exits: Vec<_> = frames
            .iter()
            .filter_map(|frame| match frame {
                ServerFrame::Event {
                    event:
                        Event::ToolEnd {
                            exit_code: Some(code),
                            ..
                        },
                    ..
                } => Some(*code),
                _ => None,
            })
            .collect();
        assert_eq!(exits, vec![1, 0]);
        assert!(
            f.exec(&["cat", "demo/README.md"])
                .await?
                .contains("two arithmetic tests pass")
        );
        assert!(
            f.exec(&["cat", "demo/arithmetic.py"])
                .await?
                .contains("return left + right")
        );
        f.exec(&["test", "!", "-d", "demo/__pycache__"]).await?;
        Ok(())
    }
    .await;
    f.remove().await?;
    result
}

#[tokio::test]
#[ignore = "requires Docker and TQA_RUNTIME_TEST_IMAGE=tqa-runtime:workstation"]
async fn rt_i_012_shell_cache_writes_use_the_private_workspace_home() -> TestResult {
    let f = DockerFixture::start(vec![shell(
        "set -eu; test \"$HOME\" = /workspace/.tqa/home; test \"$(stat -c %a \"$HOME\")\" = 700; \
         test \"$(python3 -m pip cache dir)\" = \"$HOME/.cache/pip\"; \
         test \"$(npm config get cache)\" = \"$HOME/.npm\"; npm init -y --offline >/dev/null; \
         test -n \"$(find \"$HOME/.npm/_logs\" -type f -size +0c -print -quit)\"; \
         test ! -d /workspace/.npm; test ! -d /workspace/.cache; printf 'private cache passed'"
    ), answer()]).await?;
    let result = async {
        f.gateway.send(user()).await?;
        f.wait_event("run.completed").await?;
        assert!(f.gateway.stored().iter().any(|frame| matches!(frame,
            ServerFrame::Event { event: Event::ToolResult {status: tqa_types::ToolStatus::Ok, content, ..}, ..}
            if content.contains("private cache passed")
        )), "cache check failed: {:?}", f.gateway.stored());
        Ok(())
    }.await;
    f.remove().await?;
    result
}

#[tokio::test]
#[ignore = "requires Docker and the built tqa-runtime:workstation image"]
async fn rt_i_012_workstation_toolchain_works_offline_with_production_hardening() -> TestResult {
    let output = offline::run(include_str!("support/workstation_toolchain.sh")).await?;
    assert!(output.ends_with("offline toolchain passed\n"), "{output}");
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker and the built tqa-runtime:workstation image"]
async fn sbx_c_017_workstation_browser_opens_local_html_and_saves_a_screenshot_offline()
-> TestResult {
    let script = format!(
        "node <<'JS'\n{}\nJS",
        include_str!("support/workstation_browser.cjs")
    );
    let output = offline::run(&script).await?;
    assert!(
        output.ends_with("local HTML, JavaScript and 640x480 PNG passed\n"),
        "{output}"
    );
    Ok(())
}

// TEST: RT-I-034
#[tokio::test]
#[ignore = "requires Docker and the built tqa-runtime:dev image"]
async fn rt_i_034_an_unreachable_gateway_fails_at_once_and_names_the_fix() -> TestResult {
    // A port nothing listens on stands for a fake gateway the container's namespace can't see.
    let closed = std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, 0))?
        .local_addr()?
        .port();
    let Err(error) = DockerFixture::start_with_gateway_port(vec![answer()], Some(closed)).await
    else {
        return Err("the fixture started against a closed port".into());
    };
    let error = error.to_string();
    assert!(
        error.contains(&format!("host.docker.internal:{closed}")),
        "{error}"
    );
    assert!(error.ends_with(tqa_testkit::reach::HINT), "{error}");
    Ok(())
}
