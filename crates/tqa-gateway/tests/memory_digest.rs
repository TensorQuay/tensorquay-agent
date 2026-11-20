//! GW-I-357 (ADR-0041 section 3): with long-term memory on, a completed run leaves the task's digest in its owner's
//! private folder of the workspace, `.tqa/memory/tasks/<owner>/<date>-<task>.md`: the title, the date, the files the
//! task changed and the answer's first lines. A follow-up that used no tool leaves it alone; a rename reaches its
//! first line; two people sharing a workspace each get their own folder; with memory off nothing is written.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use support::{Fixture, TestResult};
use tqa_types::{ClientFrame, ConversationId, Event, ServerFrame, ToolStatus};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const MEMORY_ON: &str = "[memory]\nenabled=true";

fn frame(seq: u64, run: &str, step: Option<u32>, event: Event) -> Result<ServerFrame> {
    Ok(ServerFrame::Event {
        seq: seq.into(),
        run_id: Some(run.parse()?),
        step,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    })
}

fn call(f: &Fixture, cookie: &str, method: Method, path: &str) -> reqwest::RequestBuilder {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
}

async fn workspace(f: &Fixture, cookie: &str) -> Result<String> {
    let created: Value = call(f, cookie, Method::POST, "/api/v1/workspaces")
        .json(&json!({"name":"Shared"}))
        .send()
        .await?
        .json()
        .await?;
    Ok(created["id"].as_str().ok_or("workspace id")?.to_owned())
}

async fn task(f: &Fixture, cookie: &str, workspace: &str, title: &str) -> Result<ConversationId> {
    let task: Value = call(f, cookie, Method::POST, "/api/v1/conversations")
        .header("idempotency-key", uuid::Uuid::now_v7().to_string())
        .json(&json!({"title":title,"model_route":"local","workspace_id":workspace}))
        .send()
        .await?
        .json()
        .await?;
    Ok(task["id"].as_str().ok_or("task id")?.parse()?)
}

async fn message(f: &Fixture, cookie: &str, id: &ConversationId, text: &str) -> TestResult {
    let sent = call(
        f,
        cookie,
        Method::POST,
        &format!("/api/v1/conversations/{id}/messages"),
    )
    .header("idempotency-key", uuid::Uuid::now_v7().to_string())
    .json(&json!({"text":text}))
    .send()
    .await?;
    assert_eq!(sent.status(), 202);
    Ok(())
}

fn root(f: &Fixture, id: &ConversationId) -> Result<PathBuf> {
    match f.backend.mounts(id).ok_or("no sandbox")?.0.source {
        tqa_sandbox::WorkspaceSource::HostDirectory(path) => Ok(path),
        _ => Err("a host-directory workspace".into()),
    }
}

/// The events of a run that works: a tool call in step 1, the answer in step 2. Nine events; the run's end follows.
fn working_events(first: u64, run: &str, answer: &[&str]) -> Result<Vec<(Option<u32>, Event)>> {
    let run_id: tqa_types::RunId = run.parse()?;
    let call_id: tqa_types::ToolCallId = format!("call-{first}").parse()?;
    let mut events = vec![
        (None, Event::RunStarted { run_id }),
        (
            Some(1),
            Event::StepStarted {
                step: 1,
                attempt: 1,
            },
        ),
        (
            Some(1),
            Event::ToolCall {
                call_id: call_id.clone(),
                name: "write_file".into(),
                arguments: json!({"path":"report.md","content":"# Report\n"}),
            },
        ),
        (
            Some(1),
            Event::ToolResult {
                call_id,
                status: ToolStatus::Ok,
                content: "written".into(),
                target: None,
                evidence: None,
            },
        ),
        (
            Some(1),
            Event::StepCompleted {
                step: 1,
                duration_ms: 1,
            },
        ),
        (
            Some(2),
            Event::StepStarted {
                step: 2,
                attempt: 1,
            },
        ),
    ];
    for text in answer {
        events.push((
            Some(2),
            Event::TextDelta {
                text: (*text).to_owned(),
            },
        ));
    }
    events.push((
        Some(2),
        Event::StepCompleted {
            step: 2,
            duration_ms: 1,
        },
    ));
    Ok(events)
}

/// A run that works: it calls a tool in step 1 (while `work` changes the workspace) and answers in step 2. Its
/// sequence numbers start at `first`.
async fn working_run(
    f: &Fixture,
    runtime: &mut support::runtime::Runtime,
    (id, first, run): (&ConversationId, u64, &str),
    answer: &[&str],
    work: impl FnOnce() -> std::io::Result<()>,
) -> TestResult {
    let mut seq = first;
    let mut worked = Some(work);
    for (step, event) in working_events(first, run, answer)? {
        if matches!(event, Event::ToolResult { .. })
            && let Some(work) = worked.take()
        {
            work()?;
        }
        runtime.send(&frame(seq, run, step, event)?).await?;
        seq += 1;
    }
    let run_id: tqa_types::RunId = run.parse()?;
    runtime
        .send(&frame(seq, run, None, Event::RunCompleted { run_id })?)
        .await?;
    f.until_sequence(id, seq).await
}

/// A run that only talks: one step, no tool.
async fn talking_run(
    f: &Fixture,
    runtime: &mut support::runtime::Runtime,
    id: &ConversationId,
    (first, run): (u64, &str),
    answer: &str,
) -> TestResult {
    let run_id: tqa_types::RunId = run.parse()?;
    let events = [
        (
            None,
            Event::RunStarted {
                run_id: run_id.clone(),
            },
        ),
        (
            Some(1),
            Event::StepStarted {
                step: 1,
                attempt: 1,
            },
        ),
        (
            Some(1),
            Event::TextDelta {
                text: answer.to_owned(),
            },
        ),
        (
            Some(1),
            Event::StepCompleted {
                step: 1,
                duration_ms: 1,
            },
        ),
        (None, Event::RunCompleted { run_id }),
    ];
    let mut seq = first;
    for (step, event) in events {
        runtime.send(&frame(seq, run, step, event)?).await?;
        seq += 1;
    }
    f.until_sequence(id, seq - 1).await
}

fn folder(root: &Path, person: uuid::Uuid) -> PathBuf {
    root.join(".tqa/memory/tasks").join(person.to_string())
}

/// The one digest in `folder`, once it reads as `wanted` says (the gateway writes it in the background).
async fn digest(folder: &Path, wanted: impl Fn(&str) -> bool) -> Result<(String, String)> {
    let mut last = String::new();
    for _ in 0..400 {
        // Hidden names are the writer's temporary file, which holds the whole text just before it is renamed.
        let files: Vec<PathBuf> = std::fs::read_dir(folder)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .filter(|path| {
                        path.file_name()
                            .is_some_and(|name| !name.to_string_lossy().starts_with('.'))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let [file] = files.as_slice()
            && let Ok(text) = std::fs::read_to_string(file)
        {
            if wanted(&text) {
                let name = file
                    .file_name()
                    .ok_or("name")?
                    .to_string_lossy()
                    .into_owned();
                return Ok((name, text));
            }
            last = text;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    Err(format!("the digest never read as wanted; last: {last:?}").into())
}

// TEST: GW-I-357
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_357_a_completed_run_leaves_the_tasks_digest_and_a_tool_less_follow_up_keeps_it(
    pool: sqlx::PgPool,
) -> TestResult {
    support::run_configured(pool, None, MEMORY_ON, async |f| {
        let (user, cookie) = f.admin().await?;
        let shared = workspace(f, &cookie).await?;
        let id = task(f, &cookie, &shared, "Write the sales report").await?;
        message(f, &cookie, &id, "Write the report").await?;
        let mut runtime = f.runtime_for(&id).await?;
        assert!(matches!(runtime.action().await?, ClientFrame::UserMessage { .. }));
        let root = root(f, &id)?;
        std::fs::write(root.join("notes.txt"), "old\n")?;
        working_run(f, &mut runtime, (&id, 1, "r-1"),
            &["The report is in report.md.\n", "\nNorth grew 12 %."],
            || {
                std::fs::write(root.join("report.md"), "# Report\n")?;
                std::fs::create_dir_all(root.join(".memory"))?;
                std::fs::write(root.join(".memory/sales.md"), "about: sales are by region\n")
            },
        )
        .await?;
        let mine = folder(&root, user.id);
        let (name, text) = digest(&mine, |text| text.contains("North grew")).await?;
        let today = time::OffsetDateTime::now_utc().date();
        let day = format!("{:04}-{:02}-{:02}", today.year(), u8::from(today.month()), today.day());
        assert_eq!(name, format!("{day}-{id}.md"), "the task's first day and its whole id");
        // notes.txt was written before the first message's record... the record is taken at delivery, so it is not
        // the task's change; memory's own folder is never listed.
        assert_eq!(
            text,
            format!(
                "# Write the sales report\ndate: {day}\nfiles: notes.txt, report.md\n\nThe report is in \
                 report.md.\nNorth grew 12 %.\n"
            )
        );
        // The private folder is not in the file view: `.tqa` is hidden as a whole.
        let listed = call(f, &cookie, Method::GET, &format!("/api/v1/conversations/{id}/files"))
            .query(&[("path", ".tqa/memory/tasks")])
            .send()
            .await?;
        assert_ne!(listed.status(), 200);

        // A thank-you run uses no tool: the digest stays as the working run left it.
        message(f, &cookie, &id, "Thanks").await?;
        assert!(matches!(runtime.action().await?, ClientFrame::UserMessage { .. }));
        talking_run(f, &mut runtime, &id, (11, "r-2"), "You're welcome.").await?;
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            assert_eq!(std::fs::read_to_string(mine.join(&name))?, text);
        }

        // Renamed by its person: the digest's first line follows, the rest stays.
        let renamed = call(f, &cookie, Method::PATCH, &format!("/api/v1/conversations/{id}"))
            .header("idempotency-key", uuid::Uuid::now_v7().to_string())
            .json(&json!({"title":"Sales by region, Q3"}))
            .send()
            .await?;
        assert_eq!(renamed.status(), 200);
        let (_, text) = digest(&mine, |text| text.starts_with("# Sales by region, Q3\n")).await?;
        assert!(text.ends_with("North grew 12 %.\n"), "{text}");

        // Another working run of the same task rewrites the same file: still one digest, files since the task began.
        message(f, &cookie, &id, "Add a chart").await?;
        assert!(matches!(runtime.action().await?, ClientFrame::UserMessage { .. }));
        working_run(f, &mut runtime, (&id, 16, "r-3"), &["Added chart.svg."], || {
            std::fs::write(root.join("chart.svg"), "<svg/>")
        })
        .await?;
        let (again, text) = digest(&mine, |text| text.contains("Added chart.svg.")).await?;
        assert_eq!(again, name);
        assert!(text.contains("files: chart.svg, notes.txt, report.md\n"), "{text}");
        Ok(())
    })
    .await
}

// TEST: GW-I-357
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_357_two_people_in_one_workspace_each_get_their_own_folder(
    pool: sqlx::PgPool,
) -> TestResult {
    support::run_configured(pool, None, MEMORY_ON, async |f| {
        let (owner, owner_cookie) = f.admin().await?;
        let (sharer, sharer_cookie) = f.member("sharer@example.invalid").await?;
        let shared = workspace(f, &owner_cookie).await?;
        let share = format!("/api/v1/workspaces/{shared}/shares/{}", sharer.id);
        let granted = call(f, &owner_cookie, Method::PUT, &share)
            .json(&json!({"access":"write"}))
            .send()
            .await?;
        assert!(granted.status().is_success());
        let first = task(f, &owner_cookie, &shared, "Review the pay rises").await?;
        let second = task(f, &sharer_cookie, &shared, "Fix the build").await?;

        message(f, &owner_cookie, &first, "Review them").await?;
        let mut runtime_a = f.runtime_for(&first).await?;
        assert!(matches!(
            runtime_a.action().await?,
            ClientFrame::UserMessage { .. }
        ));
        let root_a = root(f, &first)?;
        working_run(
            f,
            &mut runtime_a,
            (&first, 1, "r-1"),
            &["Three rises proposed."],
            || std::fs::write(root_a.join("rises.md"), "# Rises\n"),
        )
        .await?;
        let (_, theirs) = digest(&folder(&root_a, owner.id), |text| {
            text.contains("Three rises")
        })
        .await?;
        assert!(theirs.starts_with("# Review the pay rises\n"), "{theirs}");
        // The second person takes the idle workspace, and their sandbox comes up.
        message(f, &sharer_cookie, &second, "Fix it").await?;
        tokio::time::timeout(support::DEADLINE, async {
            while f.backend.created(&second).is_none() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let mut runtime_b = f.runtime_for(&second).await?;
        assert!(matches!(
            runtime_b.action().await?,
            ClientFrame::UserMessage { .. }
        ));
        let root_b = root(f, &second)?;
        assert_eq!(root_a, root_b, "one workspace");
        working_run(
            f,
            &mut runtime_b,
            (&second, 1, "r-1"),
            &["The build passes."],
            || std::fs::write(root_b.join("Makefile"), "all:\n"),
        )
        .await?;
        let (_, mine) = digest(&folder(&root_b, sharer.id), |text| {
            text.contains("build passes")
        })
        .await?;
        assert!(mine.starts_with("# Fix the build\n"), "{mine}");
        assert!(
            !mine.contains("pay rises") && !mine.contains("rises.md"),
            "{mine}"
        );
        // Each person's folder holds their own task only.
        for (person, count) in [(owner.id, 1), (sharer.id, 1)] {
            assert_eq!(std::fs::read_dir(folder(&root_b, person))?.count(), count);
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-357
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_357_with_memory_off_no_digest_is_written(pool: sqlx::PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let shared = workspace(f, &cookie).await?;
        let id = task(f, &cookie, &shared, "Write the report").await?;
        message(f, &cookie, &id, "Write it").await?;
        let mut runtime = f.runtime_for(&id).await?;
        assert!(matches!(
            runtime.action().await?,
            ClientFrame::UserMessage { .. }
        ));
        let root = root(f, &id)?;
        working_run(f, &mut runtime, (&id, 1, "r-1"), &["Done."], || {
            std::fs::write(root.join("report.md"), "# Report\n")
        })
        .await?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(!root.join(".tqa/memory").exists());
        Ok(())
    })
    .await
}

// TEST: GW-I-357
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_357_a_teams_helper_leaves_no_digest_and_its_lead_does(
    pool: sqlx::PgPool,
) -> TestResult {
    support::run_configured(pool, None, MEMORY_ON, async |f| {
        let (owner, cookie) = f.admin().await?;
        let lead = task(f, &cookie, &workspace(f, &cookie).await?, "Plan the trip").await?;
        let helper = task(f, &cookie, &workspace(f, &cookie).await?, "researcher").await?;
        // The helper is a member of the lead's team, as start_member records it.
        let (team, channel) = (uuid::Uuid::now_v7(), uuid::Uuid::now_v7());
        sqlx::query("INSERT INTO channels (id, owner_id, name, rule, kind) VALUES ($1, $2, 'Agent team', 'human', 'agent_team')")
            .bind(channel)
            .bind(owner.id)
            .execute(&f.pool)
            .await?;
        sqlx::query(
            "INSERT INTO agent_teams (id, channel_id, lead_conversation, owner_id, token_budget, deadline_at) \
             VALUES ($1, $2, $3, $4, 2000000, CURRENT_TIMESTAMP + INTERVAL '2 hours')",
        )
        .bind(team)
        .bind(channel)
        .bind(uuid::Uuid::parse_str(lead.as_str())?)
        .bind(owner.id)
        .execute(&f.pool)
        .await?;
        sqlx::query(
            "INSERT INTO agent_team_members (conversation_id, team_id, name, role, workspace, state) \
             VALUES ($1, $2, 'researcher', 'researcher', 'own_copy', 'working')",
        )
        .bind(uuid::Uuid::parse_str(helper.as_str())?)
        .bind(team)
        .execute(&f.pool)
        .await?;

        message(f, &cookie, &helper, "Find three hotels").await?;
        let mut helper_runtime = f.runtime_for(&helper).await?;
        assert!(matches!(helper_runtime.action().await?, ClientFrame::UserMessage { .. }));
        let helper_root = root(f, &helper)?;
        working_run(f, &mut helper_runtime, (&helper, 1, "r-1"), &["Three hotels found."], || {
            std::fs::write(helper_root.join("hotels.md"), "# Hotels\n")
        })
        .await?;

        message(f, &cookie, &lead, "Plan it").await?;
        let mut lead_runtime = f.runtime_for(&lead).await?;
        assert!(matches!(lead_runtime.action().await?, ClientFrame::UserMessage { .. }));
        let lead_root = root(f, &lead)?;
        working_run(f, &mut lead_runtime, (&lead, 1, "r-1"), &["The plan is in plan.md."], || {
            std::fs::write(lead_root.join("plan.md"), "# Plan\n")
        })
        .await?;
        // The lead's task is the person's: its digest comes. By then the helper's run, which ended first, has had
        // its turn too, and left none.
        let (_, text) = digest(&folder(&lead_root, owner.id), |text| text.contains("plan.md")).await?;
        assert!(text.starts_with("# Plan the trip\n"), "{text}");
        assert!(!helper_root.join(".tqa/memory").exists());
        Ok(())
    })
    .await
}
