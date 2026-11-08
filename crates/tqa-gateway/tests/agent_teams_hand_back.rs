//! GW-I-311 (a live check before 17.0, 18 Oct 2026: two helpers were stopped at their budget share mid-run, never
//! having seen the 80 % ask, and their 33 files stayed in their own copies, GW-I-304's case again): a helper stopped
//! before it reported, at its budget share or by its team's end, is handed back by the gateway. A report in the team's
//! log, in the gateway's words, carries the files it changed in its own copy (within the attachment limits, the rest
//! named), so the lead takes them as any report's, after the team's end too, and the end note lists them. A helper that
//! reported is left alone; a helper on the lead's own files has nothing to attach. Stopped at its share, a helper is
//! `stopped`, never `failed`, and isn't woken again (that check's `member_failed` every 15 s).
#[path = "support/agent_team.rs"]
mod agent_team;
#[path = "support/fixture.rs"]
mod support;
use agent_team::*;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use support::{Fixture, TestResult};
use tqa_testkit::{FakeModelServer, Script, Scripted};
use uuid::Uuid;

/// A file at most 2 KiB, a report's files 4 KiB together.
const CONFIG: &str =
    "[agent_teams]\nenabled=true\n[files]\nupload_bytes=2048\nuploads_bytes=4096\n";

/// A task's workspace directory.
async fn workspace(f: &Fixture, task: &tqa_types::ConversationId) -> Result<PathBuf> {
    let id: Uuid = sqlx::query_scalar("SELECT workspace_id FROM conversations WHERE id = $1::uuid")
        .bind(task.as_str())
        .fetch_one(&f.pool)
        .await?;
    let path = f
        .config_storage_root()
        .join("workspaces")
        .join(id.to_string());
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn write(root: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

/// A file as the copy left it: written before the helper's first run, so not one of its changes.
fn copied(root: &Path, path: &str) -> Result<()> {
    write(root, path, b"from the lead's files")?;
    let file = std::fs::File::options().write(true).open(root.join(path))?;
    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_hours(1))?;
    Ok(())
}

/// The gateway's hand-back reports for `member` in the team's log: their seq and body.
async fn handed_back(
    f: &Fixture,
    team: Uuid,
    member: &tqa_types::ConversationId,
) -> Result<Vec<(i64, Value)>> {
    Ok(sqlx::query_as(
        "SELECT m.seq, m.body FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
         WHERE t.id = $1 AND m.type = 'report' AND m.author_kind = 'gateway' AND m.author_id = $2 ORDER BY m.seq",
    )
    .bind(team)
    .bind(member.as_str())
    .fetch_all(&f.pool)
    .await?)
}

/// The hand-back for `member`, once it is in the log.
async fn until_handed_back(
    f: &Fixture,
    team: Uuid,
    member: &tqa_types::ConversationId,
) -> Result<(i64, Value)> {
    for _ in 0..100 {
        if let Some(found) = handed_back(f, team, member).await?.into_iter().next() {
            return Ok(found);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err(format!("{member} was never handed back").into())
}

fn names(body: &Value) -> Vec<String> {
    let mut names: Vec<String> = body["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| file["name"].as_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

async fn member_state(f: &Fixture, member: &tqa_types::ConversationId) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM agent_team_members WHERE conversation_id = $1::uuid")
            .bind(member.as_str())
            .fetch_one(&f.pool)
            .await?,
    )
}

async fn until_ended(f: &Fixture, team: Uuid) -> TestResult {
    for _ in 0..100 {
        let state: String = sqlx::query_scalar("SELECT state FROM agent_teams WHERE id = $1")
            .bind(team)
            .fetch_one(&f.pool)
            .await?;
        if state == "ended" {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err("the team didn't end".into())
}

/// The lead's `take_files` from its run `r-1`: status and body.
async fn take(f: &Fixture, lead_token: &str, key: &str, seq: i64) -> Result<(u16, Value)> {
    let response = f
        .client
        .post(f.url("/runtime/teams/files"))
        .bearer_auth(lead_token)
        .header("idempotency-key", key)
        .header("x-tqa-run-id", "r-1")
        .json(&json!({"report_seq": seq}))
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await.unwrap_or(Value::Null)))
}

// TEST: GW-I-311
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_311_a_team_end_hands_back_what_an_unreported_helper_changed(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let t = team(f).await?;
        // researcher never reports: two files of its own, one as copied, one past the 2 KiB limit.
        let mine = workspace(f, &t.researcher.0).await?;
        copied(&mine, "old.txt")?;
        write(&mine, "notes.md", b"# Kyoto\nDay 1: temples.\n")?;
        write(&mine, "sub/data.csv", b"day,place\n1,Kinkaku-ji\n")?;
        write(&mine, "big.bin", &[0_u8; 3000])?;
        // reviewer reported: nothing is handed back for it.
        let (status, answer) = send(f, &t.reviewer.1, "r-3:c-1", &json!({"type": "report", "text": "Checked."})).await?;
        assert_eq!(status, 201, "{answer}");
        let (status, answer) = done(f, &t.lead.1, "r-1:c-1", &json!({"summary": "Trip planned."})).await?;
        assert_eq!(status, 200, "{answer}");
        until_ended(f, t.id).await?;
        let (seq, body) = until_handed_back(f, t.id, &t.researcher.0).await?;
        assert_eq!(names(&body), ["notes.md", "sub/data.csv"], "its changes within the limits, nothing as copied");
        let text = body["text"].as_str().unwrap_or_default();
        assert!(text.starts_with("The gateway"), "the gateway's words, never the helper's: {text}");
        assert!(text.contains("researcher stopped before it reported"), "{text}");
        assert!(text.contains("big.bin"), "a file over the limits is named: {text}");
        assert!(text.contains("2 KB"), "the limit is named: {text}");
        assert!(handed_back(f, t.id, &t.reviewer.0).await?.is_empty(), "reviewer reported");
        assert_eq!(member_state(f, &t.researcher.0).await?, "stopped", "a hand-back isn't its report");
        assert_eq!(member_state(f, &t.reviewer.0).await?, "done");
        // The end note lists the handed-back files for the person.
        let note: Value = sqlx::query_scalar(
            "SELECT m.body FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
             WHERE t.id = $1 AND m.type = 'ended'",
        )
        .bind(t.id)
        .fetch_one(&f.pool)
        .await?;
        assert!(note.to_string().contains("notes.md"), "{note}");
        // After the end, the lead still takes them, as any report's files.
        let (status, taken) = take(f, &t.lead.1, "take-1", seq).await?;
        assert_eq!(status, 200, "{taken}");
        let lead = workspace(f, &t.lead.0).await?;
        assert!(lead.join("uploads/researcher-notes.md").is_file(), "{taken}");
        Ok(())
    })
    .await
}

// TEST: GW-I-311
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_311_only_the_work_since_its_last_assignment_is_handed_back(
    pool: PgPool,
) -> TestResult {
    // Found on 18 Oct: counted from its first run, a reassigned helper's hand-back attached what it had
    // already reported, which the lead may have taken and changed since; taking it again would overwrite that.
    support::run_configured(pool, None, CONFIG, async |f| {
        let t = team(f).await?;
        let mine = workspace(f, &t.researcher.0).await?;
        write(&mine, "first.md", b"The first part.\n")?;
        let (status, answer) = send(
            f,
            &t.researcher.1,
            "r-2:c-1",
            &json!({"type": "report", "text": "The first part is in first.md.", "files": ["first.md"]}),
        )
        .await?;
        assert_eq!(status, 201, "{answer}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let (status, answer) = send(
            f,
            &t.lead.1,
            "r-1:c-1",
            &json!({"type": "assign", "to": "researcher", "text": "Now the second part."}),
        )
        .await?;
        assert_eq!(status, 201, "{answer}");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        write(&mine, "second.md", b"The second part.\n")?;
        let (status, answer) = done(f, &t.lead.1, "r-1:c-2", &json!({"summary": "Done."})).await?;
        assert_eq!(status, 200, "{answer}");
        until_ended(f, t.id).await?;
        let (_, body) = until_handed_back(f, t.id, &t.researcher.0).await?;
        assert_eq!(names(&body), ["second.md"], "only what it changed since its last assignment");
        let text = body["text"].as_str().unwrap_or_default();
        assert!(text.contains("Files it deleted aren't listed."), "{text}");
        Ok(())
    })
    .await
}

// TEST: GW-I-311
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_311_the_runtimes_own_files_are_never_handed_back_and_each_file_left_says_why(
    pool: PgPool,
) -> TestResult {
    // A live check before 17.0 (18 Oct 2026): the hand-back attached nothing and named 28 files of the runtime's own
    // `.tqa` (its tool-output spill, fetched copies) as "over the attachment limits of 25 MB": neither the helper's
    // work nor over any limit.
    use std::os::unix::fs::PermissionsExt as _;
    support::run_configured(pool, None, CONFIG, async |f| {
        let t = team(f).await?;
        let mine = workspace(f, &t.researcher.0).await?;
        write(
            &mine,
            ".tqa/01a14b5f/spill/chatcmpl-tool-1.txt",
            b"a tool's long output",
        )?;
        write(&mine, "trip.md", b"# Kyoto in 3 days\n")?;
        write(&mine, "locked.md", b"unreadable")?;
        std::fs::set_permissions(
            mine.join("locked.md"),
            std::fs::Permissions::from_mode(0o000),
        )?;
        let (status, answer) = done(f, &t.lead.1, "r-1:c-1", &json!({"summary": "Done."})).await?;
        assert_eq!(status, 200, "{answer}");
        until_ended(f, t.id).await?;
        let (_, body) = until_handed_back(f, t.id, &t.researcher.0).await?;
        std::fs::set_permissions(
            mine.join("locked.md"),
            std::fs::Permissions::from_mode(0o644),
        )?;
        assert_eq!(names(&body), ["trip.md"], "only the helper's own work");
        let text = body["text"].as_str().unwrap_or_default();
        assert!(
            !text.contains(".tqa"),
            "the runtime's own files are never named: {text}"
        );
        assert!(
            text.contains("Couldn't be read (1): locked.md"),
            "the true reason: {text}"
        );
        assert!(
            !text.contains("limits"),
            "no file is over a limit here: {text}"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-311
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_311_a_helper_on_the_leads_files_has_nothing_to_attach(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let t = team(f).await?;
        sqlx::query(
            "UPDATE agent_team_members SET workspace = 'my_files' WHERE conversation_id = $1::uuid",
        )
        .bind(t.researcher.0.as_str())
        .execute(&f.pool)
        .await?;
        let (status, answer) = done(f, &t.lead.1, "r-1:c-1", &json!({"summary": "Done."})).await?;
        assert_eq!(status, 200, "{answer}");
        until_ended(f, t.id).await?;
        let (_, body) = until_handed_back(f, t.id, &t.researcher.0).await?;
        assert!(names(&body).is_empty());
        let text = body["text"].as_str().unwrap_or_default();
        assert!(
            text.starts_with("The gateway") && text.contains("your own files"),
            "{text}"
        );
        Ok(())
    })
    .await
}

fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='pinned-model'\nauth={{kind='bearer'}}\nkey_ref='secrets/local'\nstream_usage=true",
        server.base_url()
    ))?)
}

/// A model server answering once, reporting 40 tokens.
async fn server() -> Result<FakeModelServer> {
    let script = Script::new(vec![
        Scripted::Text("done".into()),
        Scripted::Usage(tqa_llm::Usage {
            prompt_tokens: 30,
            completion_tokens: 10,
            cached_tokens: None,
            reasoning_tokens: None,
        }),
    ]);
    Ok(FakeModelServer::start(vec![script], Arc::new(tqa_llm::TokioSleeper)).await?)
}

/// One model request from `conversation`'s runtime: its status.
async fn call(f: &Fixture, conversation: &tqa_types::ConversationId) -> Result<u16> {
    let token = f
        .gateway
        .runtime_tokens()
        .mint(
            conversation,
            &format!("sandbox-{}", Uuid::now_v7().simple()).parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    let response = f
        .client
        .post(f.url("/relay/r/local/chat/completions"))
        .bearer_auth(token.expose_secret())
        .json(&json!({"model":"m","messages":[{"role":"user","content":"hi"}],"stream":true}))
        .send()
        .await?;
    let status = response.status().as_u16();
    let _ = response.text().await?;
    Ok(status)
}

// TEST: GW-I-311
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_311_at_its_budget_share_a_helper_is_stopped_and_handed_back(
    pool: PgPool,
) -> TestResult {
    let model = server().await?;
    support::run_configured(pool, Some(registry(&model)?), CONFIG, async |f| {
        let t = team(f).await?;
        let secret = secrecy::SecretString::from(format!("synthetic-provider-{}", Uuid::now_v7()));
        f.gateway.secret_store().put(&"secrets/local".parse()?, secret).await?;
        // 880 of 1000 spent: the lead's answer (40) takes the team past the helpers' share (90 %), short of the end.
        sqlx::query("UPDATE agent_teams SET token_budget = 1000, spent_tokens = 880 WHERE id = $1")
            .bind(t.id)
            .execute(&f.pool)
            .await?;
        let mine = workspace(f, &t.researcher.0).await?;
        write(&mine, "findings.md", b"Hotels: three options.\n")?;
        assert_eq!(call(f, &t.lead.0).await?, 200);
        let (_, body) = until_handed_back(f, t.id, &t.researcher.0).await?;
        assert_eq!(names(&body), ["findings.md"]);
        assert!(body["text"].as_str().unwrap_or_default().contains("budget"), "{body}");
        assert_eq!(member_state(f, &t.researcher.0).await?, "stopped", "stopped, never failed");
        assert_eq!(member_state(f, &t.reviewer.0).await?, "stopped");
        let state: String = sqlx::query_scalar("SELECT state FROM agent_teams WHERE id = $1")
            .bind(t.id)
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(state, "running", "the rest of the budget is the lead's");
        // No wake for a stopped helper, so no failed run and no member_failed (W1: one every 15 s).
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let failed: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM channel_messages m JOIN agent_teams t ON t.channel_id = m.channel_id \
             WHERE t.id = $1 AND m.type = 'notice' AND m.body->>'kind' = 'member_failed'",
        )
        .bind(t.id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(failed, 0);
        Ok(())
    })
    .await
}
