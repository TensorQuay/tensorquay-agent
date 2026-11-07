//! GW-I-110 and SEC-038's files (ADR-0034 point 6), on Docker: a member's report reads its files from the member's
//! workspace, checked and capped as attachments are, and stores them per member task; only the lead places them, in
//! its own `uploads/` as `<member>-<path>`, never overwriting; a deleted member's files can't be taken and go with its
//! purge.
#[path = "support/fixture.rs"]
mod support;
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::PathBuf;
use support::{Fixture, TestResult};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// A file at most 2 KiB, a report's files 4 KiB together.
const CONFIG: &str =
    "[agent_teams]\nenabled=true\n[files]\nupload_bytes=2048\nuploads_bytes=4096\n";

/// The run every call comes from.
const RUN: &str = "r-1";

/// A lead task, its member `researcher` and a second team's lead, each with its own workspace; the owner's cookie.
struct Team {
    lead: Uuid,
    member: Uuid,
    other_lead: Uuid,
    cookie: String,
}

async fn team(f: &Fixture) -> Result<Team> {
    let (owner, cookie) = f.admin().await?;
    let lead = Uuid::parse_str(f.conversation(owner.id).await?.id.as_str())?;
    let member = Uuid::parse_str(f.conversation(owner.id).await?.id.as_str())?;
    let other_lead = Uuid::parse_str(f.conversation(owner.id).await?.id.as_str())?;
    // Each task's current run is `r-1`, as its runtime's calls name it.
    for task in [lead, member, other_lead] {
        f.gateway
            .events()
            .append(&task.to_string().parse()?, support::started(1, RUN)?)
            .await?;
    }
    for (leader, name) in [(lead, Some("researcher")), (other_lead, None)] {
        let (id, channel) = (Uuid::now_v7(), Uuid::now_v7());
        sqlx::query("INSERT INTO channels (id, owner_id, name, rule, kind) VALUES ($1, $2, 'Agent team', 'human', 'agent_team')")
            .bind(channel)
            .bind(owner.id)
            .execute(&f.pool)
            .await?;
        sqlx::query(
            "INSERT INTO agent_teams (id, channel_id, lead_conversation, owner_id, token_budget, deadline_at) \
             VALUES ($1, $2, $3, $4, 2000000, CURRENT_TIMESTAMP + interval '1 hour')",
        )
        .bind(id)
        .bind(channel)
        .bind(leader)
        .bind(owner.id)
        .execute(&f.pool)
        .await?;
        if let Some(name) = name {
            sqlx::query(
                "INSERT INTO agent_team_members (conversation_id, team_id, name, role, workspace, state) \
                 VALUES ($1, $2, $3, $3, 'own_copy', 'working')",
            )
            .bind(member)
            .bind(id)
            .bind(name)
            .execute(&f.pool)
            .await?;
        }
    }
    Ok(Team {
        lead,
        member,
        other_lead,
        cookie,
    })
}

/// A task's workspace directory.
async fn workspace(f: &Fixture, task: Uuid) -> Result<PathBuf> {
    let id: Uuid = sqlx::query_scalar("SELECT workspace_id FROM conversations WHERE id = $1")
        .bind(task)
        .fetch_one(&f.pool)
        .await?;
    let path = f
        .config_storage_root()
        .join("workspaces")
        .join(id.to_string());
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn write(root: &std::path::Path, path: &str, bytes: &[u8]) -> Result<()> {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

/// `task`'s runtime token: its linked sandbox's when it has one (a new token would replace that link), else a new one.
async fn token(f: &Fixture, task: Uuid) -> Result<String> {
    let id: tqa_types::ConversationId = task.to_string().parse()?;
    if let Some(token) = f.backend.token(&id) {
        return Ok(token.expose_secret().to_owned());
    }
    let token = f
        .gateway
        .runtime_tokens()
        .mint(
            &id,
            &format!("sandbox-{}", Uuid::now_v7().simple()).parse()?,
            std::net::Ipv4Addr::LOCALHOST.into(),
        )
        .await?;
    Ok(token.expose_secret().to_owned())
}

/// A runtime call as `task` from its current run: its status and body.
async fn call(
    f: &Fixture,
    task: Uuid,
    route: &str,
    key: &str,
    body: &Value,
) -> Result<(u16, Value)> {
    let response = f
        .client
        .post(f.url(route))
        .bearer_auth(token(f, task).await?)
        .header("idempotency-key", key)
        .header("x-tqa-run-id", RUN)
        .json(body)
        .send()
        .await?;
    let status = response.status().as_u16();
    Ok((status, response.json().await?))
}

async fn report(f: &Fixture, member: Uuid, key: &str, files: &[&str]) -> Result<(u16, Value)> {
    let body = json!({"type": "report", "text": "Done; the files are attached.", "files": files});
    call(f, member, "/runtime/teams/messages", key, &body).await
}

async fn take(f: &Fixture, lead: Uuid, key: &str, body: &Value) -> Result<(u16, Value)> {
    call(f, lead, "/runtime/teams/files", key, body).await
}

/// A refusal's code and the file its details name.
fn refused(answer: &Value) -> (Value, Value) {
    (
        answer["error"]["code"].clone(),
        answer["error"]["details"]["path"].clone(),
    )
}

/// The member's stored report folders and leftover staging folders, once no removal is pending.
async fn stored(f: &Fixture, member: Uuid) -> Result<Vec<String>> {
    let root = f
        .config_storage_root()
        .join("handed")
        .join(member.to_string());
    tokio::time::timeout(support::DEADLINE, async {
        loop {
            let names: Vec<String> = match std::fs::read_dir(&root) {
                Ok(entries) => entries
                    .filter_map(std::result::Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect(),
                Err(_) => Vec::new(),
            };
            if !names.iter().any(|name| name.starts_with(".staging-")) {
                return Ok::<_, Box<dyn std::error::Error>>(names);
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| "a staging folder was never removed")?
}

// TEST: GW-I-110
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_110_a_reports_files_are_stored_and_the_lead_places_them_in_uploads_without_overwriting(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let t = team(f).await?;
        let (member_root, lead_root) = (workspace(f, t.member).await?, workspace(f, t.lead).await?);
        write(&member_root, "src/report.md", b"# Q3\nRevenue rose.\n")?;
        write(&member_root, "data.csv", b"quarter,revenue\nQ3,12\n")?;
        // A name the lead's uploads already hold: the placed copy takes the next free one.
        write(&lead_root, "uploads/researcher-data.csv", b"the lead's own")?;

        let (status, sent) = report(f, t.member, "r1", &["src/report.md", "data.csv"]).await?;
        assert_eq!(status, 201, "{sent}");
        let seq = sent["seq"].as_u64().ok_or("no seq")?;
        let body: Value = sqlx::query_scalar(
            "SELECT body FROM channel_messages WHERE seq = $1 AND type = 'report' AND author_id = $2",
        )
        .bind(i64::try_from(seq)?)
        .bind(t.member.to_string())
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(
            body["files"],
            json!([
                {"name": "src/report.md", "size": 19,
                 "sha256": "baf8f34b6cfc960ddca83cb48f92845bf61fc7c061a4e6b6bec05e1a1ad003f5"},
                {"name": "data.csv", "size": 22,
                 "sha256": "925826bd6bb66c09c9cbf150ed943feb97ff0ef20a754469be7d3ddf6c449251"}
            ]),
            "the report lists each file's path, size and SHA-256"
        );
        assert_eq!(stored(f, t.member).await?, vec![seq.to_string()]);

        let (status, placed) = take(f, t.lead, "t1", &json!({"report_seq": seq})).await?;
        assert_eq!(status, 200, "{placed}");
        assert_eq!(
            placed,
            json!({"placed": [
                {"name": "src/report.md", "path": "uploads/researcher-src-report.md", "size": 19},
                {"name": "data.csv", "path": "uploads/researcher-data-2.csv", "size": 22}
            ]})
        );
        assert_eq!(std::fs::read(lead_root.join("uploads/researcher-src-report.md"))?, b"# Q3\nRevenue rose.\n");
        assert_eq!(std::fs::read(lead_root.join("uploads/researcher-data.csv"))?, b"the lead's own", "never overwritten");
        // The same key answers as the first call did, and places nothing more.
        assert_eq!(take(f, t.lead, "t1", &json!({"report_seq": seq})).await?, (200, placed));
        assert!(!lead_root.join("uploads/researcher-src-report-2.md").exists());
        // Taking one again, by its path in the report: another copy.
        let (status, again) = take(f, t.lead, "t2", &json!({"report_seq": seq, "files": ["src/report.md"]})).await?;
        assert_eq!(status, 200, "{again}");
        assert_eq!(again["placed"][0]["path"], json!("uploads/researcher-src-report-2.md"));

        let (status, missing) = take(f, t.lead, "t3", &json!({"report_seq": seq, "files": ["notes.txt"]})).await?;
        assert_eq!((status, refused(&missing)), (404, (json!("not_found"), json!("notes.txt"))));
        let (status, unknown) = take(f, t.lead, "t4", &json!({"report_seq": seq + 100})).await?;
        assert_eq!((status, unknown["error"]["code"].clone()), (422, json!("unknown_reference")));
        // The call names the lead's current run (the S1 contract's headers), or nothing is placed.
        for (run, status, code) in [(None, 400, "invalid_request"), (Some("r-2"), 409, "not_current_run")] {
            let request = f
                .client
                .post(f.url("/runtime/teams/files"))
                .bearer_auth(token(f, t.lead).await?)
                .header("idempotency-key", format!("run-{status}"))
                .json(&json!({"report_seq": seq}));
            let request = match run {
                Some(run) => request.header("x-tqa-run-id", run),
                None => request,
            };
            let response = request.send().await?;
            assert_eq!(response.status().as_u16(), status);
            assert_eq!(response.json::<Value>().await?["error"]["code"], json!(code));
        }
        assert!(!lead_root.join("uploads/researcher-src-report-3.md").exists());
        Ok(())
    })
    .await
}

// TEST: GW-I-110
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_110_caps_links_and_paths_are_refused_naming_the_file_and_nothing_is_kept(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let t = team(f).await?;
        let root = workspace(f, t.member).await?;
        write(&root, "small.txt", &[b'a'; 1500])?;
        write(&root, "other.txt", &[b'b'; 1500])?;
        write(&root, "third.txt", &[b'c'; 1500])?;
        write(&root, "large.bin", &[0; 2049])?;
        write(&root, "dir/inner.txt", b"x")?;
        std::os::unix::fs::symlink("/etc/hostname", root.join("escape"))?;
        std::os::unix::fs::symlink(root.join("small.txt"), root.join("inside-link"))?;
        for (n, (files, status, code, path)) in [
            (
                vec!["large.bin"],
                413,
                "upload_too_large",
                json!("large.bin"),
            ),
            (
                vec!["small.txt", "other.txt", "third.txt"],
                409,
                "uploads_full",
                json!("third.txt"),
            ),
            (vec!["escape"], 400, "path_invalid", json!("escape")),
            (
                vec!["inside-link"],
                400,
                "path_invalid",
                json!("inside-link"),
            ),
            (vec!["dir"], 400, "path_invalid", json!("dir")),
            (vec!["gone.txt"], 404, "not_found", json!("gone.txt")),
            (vec!["../x"], 400, "invalid_request", Value::Null),
            (vec!["/etc/passwd"], 400, "invalid_request", Value::Null),
        ]
        .into_iter()
        .enumerate()
        {
            let (got, answer) = report(f, t.member, &format!("bad-{n}"), &files).await?;
            assert_eq!(
                (got, refused(&answer)),
                (status, (json!(code), path)),
                "{files:?}: {answer}"
            );
        }
        let many: Vec<String> = (0..33).map(|n| format!("f{n}.txt")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(report(f, t.member, "many", &many).await?.0, 400);
        assert_eq!(
            stored(f, t.member).await?,
            Vec::<String>::new(),
            "a refused report keeps no copy"
        );
        let reports: i64 =
            sqlx::query_scalar("SELECT count(*) FROM channel_messages WHERE type = 'report'")
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(reports, 0, "and stores no report");
        Ok(())
    })
    .await
}

// TEST: GW-I-110
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_110_a_deleted_members_files_cant_be_taken_and_go_with_its_purge(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let t = team(f).await?;
        write(&workspace(f, t.member).await?, "result.md", b"result")?;
        let (status, sent) = report(f, t.member, "r1", &["result.md"]).await?;
        assert_eq!(status, 201, "{sent}");
        let seq = sent["seq"].as_u64().ok_or("no seq")?;
        let deleted = f
            .client
            .delete(f.url(&format!("/api/v1/conversations/{}", t.member)))
            .header("origin", f.origin())
            .header("cookie", &t.cookie)
            .send()
            .await?;
        assert_eq!(deleted.status().as_u16(), 204);
        let (status, answer) = take(f, t.lead, "t1", &json!({"report_seq": seq})).await?;
        assert_eq!(
            (status, refused(&answer)),
            (404, (json!("not_found"), json!("result.md")))
        );
        f.gateway.purge_now().await?;
        assert!(
            !f.config_storage_root()
                .join("handed")
                .join(t.member.to_string())
                .exists(),
            "the stored copies go with the member task"
        );
        Ok(())
    })
    .await
}

// TEST: SEC-038
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_038_handed_back_files_reach_only_their_own_lead(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, CONFIG, async |f| {
        let t = team(f).await?;
        write(&workspace(f, t.member).await?, "result.md", b"result")?;
        let (_, sent) = report(f, t.member, "r1", &["result.md"]).await?;
        let seq = sent["seq"].as_u64().ok_or("no seq")?;
        let request = json!({"report_seq": seq});
        let (status, answer) = take(f, t.member, "m1", &request).await?;
        assert_eq!(
            (status, answer["error"]["code"].clone()),
            (403, json!("not_lead"))
        );
        // Another team's lead names this report's seq: it is not a report of its own team.
        let (status, answer) = take(f, t.other_lead, "o1", &request).await?;
        assert_eq!(
            (status, answer["error"]["code"].clone()),
            (422, json!("unknown_reference"))
        );
        assert!(
            !workspace(f, t.other_lead).await?.join("uploads").exists(),
            "nothing reached another team's lead"
        );
        Ok(())
    })
    .await
}

/// Starts a runtime call as `task`, for a test that answers the fake runtime meanwhile.
async fn start(
    f: &Fixture,
    task: Uuid,
    route: &str,
    key: &str,
    body: &Value,
) -> Result<tokio::task::JoinHandle<std::result::Result<(u16, Value), reqwest::Error>>> {
    let request = f
        .client
        .post(f.url(route))
        .bearer_auth(token(f, task).await?)
        .header("idempotency-key", key)
        .header("x-tqa-run-id", RUN)
        .json(body);
    Ok(tokio::spawn(async move {
        let response = request.send().await?;
        let status = response.status().as_u16();
        Ok((status, response.json().await?))
    }))
}

fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// TEST: GW-I-110
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_110_on_the_microvm_files_are_read_and_placed_through_the_workspace_service(
    pool: PgPool,
) -> TestResult {
    use support::workspace_link as link;
    use tqa_types::WorkspaceOp;
    let config = format!(
        "[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\n{CONFIG}",
        "b".repeat(64)
    );
    support::run_configured(pool, None, &config, async |f| {
        let t = team(f).await?;
        let mut member = f.runtime_for(&t.member.to_string().parse()?).await?;
        let mut lead = f.runtime_for(&t.lead.to_string().parse()?).await?;
        let bytes = b"# Q3\n";

        // The report reads its file from the member's runtime, chunk by chunk.
        let body = json!({"type": "report", "text": "Done.", "files": ["src/report.md"]});
        let pending = start(f, t.member, "/runtime/teams/messages", "r1", &body).await?;
        let (asked, op) = link::request(&mut member).await?;
        let WorkspaceOp::Read { path, offset, .. } = op else {
            return Err(format!("expected a read, got {op:?}").into());
        };
        assert_eq!((path.as_str(), offset), ("src/report.md", 0));
        link::reply(&mut member, asked, &[link::content(&base64(bytes), 5, true)]).await?;
        let (status, sent) = pending.await??;
        assert_eq!(status, 201, "{sent}");
        let seq = sent["seq"].as_u64().ok_or("no seq")?;

        // A file over the limit is refused from its first chunk's size, naming it.
        let body = json!({"type": "report", "text": "Done.", "files": ["big.bin"]});
        let pending = start(f, t.member, "/runtime/teams/messages", "r2", &body).await?;
        let (asked, _) = link::request(&mut member).await?;
        link::reply(&mut member, asked, &[link::content(&base64(b"x"), 3000, false)]).await?;
        let (status, refusal) = pending.await??;
        assert_eq!((status, refused(&refusal)), (413, (json!("upload_too_large"), json!("big.bin"))));

        // The lead's take uploads it through the lead's runtime, as an attachment.
        let pending = start(f, t.lead, "/runtime/teams/files", "t1", &json!({"report_seq": seq})).await?;
        let (asked, op) = link::request(&mut lead).await?;
        assert!(matches!(op, WorkspaceOp::List { ref path, .. } if path == "uploads"), "{op:?}");
        link::reply(&mut lead, asked, &[json!({"kind": "listing", "items": []})]).await?;
        let (asked, op) = link::request(&mut lead).await?;
        let WorkspaceOp::Upload { name, offset, data, last, .. } = op else {
            return Err(format!("expected an upload chunk, got {op:?}").into());
        };
        assert_eq!((name.as_str(), offset, data, last), ("researcher-src-report.md", 0, base64(bytes), true));
        let file = json!({"path": "uploads/researcher-src-report.md", "name": "researcher-src-report.md", "size": 5});
        link::reply(&mut lead, asked, &[json!({"kind": "uploaded", "received": 5, "file": file})]).await?;
        let (status, placed) = pending.await??;
        assert_eq!(status, 200, "{placed}");
        assert_eq!(
            placed,
            json!({"placed": [{"name": "src/report.md", "path": "uploads/researcher-src-report.md", "size": 5}]})
        );
        Ok(())
    })
    .await
}
