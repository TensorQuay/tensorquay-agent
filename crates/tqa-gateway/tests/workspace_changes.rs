//! GW-I-084 (ADR-0030): run changes on a block-image workspace, read through its runtime's manifest and blobs, answer
//! as on Docker; a failed or slow snapshot never holds a message back. SEC-027: forged blobs are never stored.
#[path = "support/fixture.rs"]
mod support;

use std::collections::BTreeMap;

use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use support::{Fixture, TestResult, runtime::Runtime, workspace_link as link};
use tqa_types::{ClientFrame, WorkspaceOp};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Files = BTreeMap<String, Vec<u8>>;

fn firecracker(extra: &str) -> String {
    format!(
        "[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\n{extra}",
        "b".repeat(64)
    )
}

fn files(entries: &[(&str, &str)]) -> Files {
    entries
        .iter()
        .map(|(path, text)| ((*path).to_owned(), text.as_bytes().to_vec()))
        .collect()
}

async fn message(f: &Fixture, cookie: &str, id: &str, text: &str) -> TestResult {
    let accepted = f
        .action(
            &format!("/api/v1/conversations/{id}/messages"),
            cookie,
            Some(&uuid::Uuid::now_v7().to_string()),
            &json!({"text": text, "attachments": []}),
        )
        .await?;
    assert_eq!(accepted.status(), 202);
    Ok(())
}

/// Answers a delivery's snapshot from `files` until the message arrives; returns the hashes each blobs request named.
async fn delivered(runtime: &mut Runtime, files: &Files) -> Result<Vec<Vec<String>>> {
    let mut asked = Vec::new();
    for _ in 0..16 {
        match runtime.action().await? {
            ClientFrame::WorkspaceRequest { request_id, op } => {
                if let WorkspaceOp::Blobs { hashes } = &op {
                    asked.push(hashes.clone());
                }
                link::answer(runtime, request_id, &op, files).await?;
            }
            ClientFrame::UserMessage { .. } => return Ok(asked),
            ClientFrame::WorkspaceCancel { .. } => {}
            other => return Err(format!("unexpected action {other:?}").into()),
        }
    }
    Err("the message never arrived".into())
}

fn run_event(sequence: u64, run: &str, event: tqa_types::Event) -> Result<tqa_types::ServerFrame> {
    Ok(tqa_types::ServerFrame::Event {
        seq: sequence.into(),
        run_id: Some(run.parse()?),
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    })
}

/// Reports a run from start to end at sequences `first` and `first + 1`.
async fn run(
    f: &Fixture,
    runtime: &mut Runtime,
    id: &tqa_types::ConversationId,
    (first, run): (u64, &str),
) -> TestResult {
    let run_id: tqa_types::RunId = run.parse()?;
    let started = tqa_types::Event::RunStarted {
        run_id: run_id.clone(),
    };
    runtime.send(&run_event(first, run, started)?).await?;
    let completed = tqa_types::Event::RunCompleted { run_id };
    runtime.send(&run_event(first + 1, run, completed)?).await?;
    f.until_sequence(id, first + 1).await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_084_run_changes_come_from_the_runtimes_snapshots(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = conversation.id.clone();
        let mut runtime = f.runtime(&conversation).await?;
        let mut workspace = files(&[("notes.txt", "one\n"), ("tool.bin", "\0\u{1}")]);
        // The delivery's snapshot copies the text file, not the binary one.
        message(f, &cookie, id.as_str(), "Write the report").await?;
        let asked = delivered(&mut runtime, &workspace).await?;
        assert_eq!(asked, [vec![link::sha(b"one\n")]]);
        // The run changes the workspace; the latest run is compared with the live workspace.
        run(f, &mut runtime, &id, (1, "r-1")).await?;
        workspace = files(&[("notes.txt", "one\ntwo\n"), ("report.md", "# Report\n"), ("tool.bin", "\0\u{1}")]);
        let changes = format!("/api/v1/conversations/{id}/runs/r-1/changes");
        let pending = link::get(f, &changes, &cookie);
        assert!(matches!(link::serve(&mut runtime, &workspace).await?, WorkspaceOp::Manifest { .. }));
        let WorkspaceOp::Blobs { hashes } = link::serve(&mut runtime, &workspace).await? else {
            return Err("expected the new copies".into());
        };
        assert_eq!(hashes, [link::sha(b"one\ntwo\n"), link::sha(b"# Report\n")]);
        let latest = pending.await??.json()?;
        let expected = json!([
            {"path":"notes.txt","status":"modified","added":1,"removed":0,"binary":false,"too_large":false},
            {"path":"report.md","status":"added","added":1,"removed":0,"binary":false,"too_large":false},
        ]);
        assert_eq!((&latest["files"], &latest["live"]), (&expected, &json!(true)), "{latest}");
        // A diff reads the stored copies; nothing already stored is asked for again.
        let diff = link::get(f, &format!("{changes}/diff?path=notes.txt"), &cookie);
        assert!(matches!(link::serve(&mut runtime, &workspace).await?, WorkspaceOp::Manifest { .. }));
        let diff = diff.await??.json()?;
        assert_eq!(diff["unified"], "--- a/notes.txt\n+++ b/notes.txt\n@@ -1 +1,2 @@\n one\n+two\n", "{diff}");
        // The next message's snapshot closes r-1: stored copies are not fetched again.
        message(f, &cookie, id.as_str(), "Again").await?;
        assert!(delivered(&mut runtime, &workspace).await?.is_empty());
        let closed = f
            .client
            .request(Method::GET, f.url(&changes))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        assert_eq!((&closed["files"], &closed["live"]), (&expected, &json!(false)), "{closed}");
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_084_a_failed_or_slow_snapshot_never_holds_a_message_back(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker("[files]\nsnapshot_seconds=1\n"), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        // The runtime refuses the manifest: the message still goes.
        message(f, &cookie, conversation.id.as_str(), "First").await?;
        let (asked, op) = link::request(&mut runtime).await?;
        assert!(matches!(op, WorkspaceOp::Manifest { .. }));
        let refused = json!({"kind": "error", "code": "internal", "message": "broken", "retryable": false});
        link::reply(&mut runtime, asked, &[refused]).await?;
        assert!(matches!(link::next_action(&mut runtime).await?, ClientFrame::UserMessage { .. }));
        // The runtime never answers: the message goes once the snapshot's bound passes.
        message(f, &cookie, conversation.id.as_str(), "Second").await?;
        let (_, op) = link::request(&mut runtime).await?;
        assert!(matches!(op, WorkspaceOp::Manifest { .. }));
        assert!(matches!(link::next_action(&mut runtime).await?, ClientFrame::UserMessage { .. }));
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_027_forged_blobs_are_never_stored(pool: PgPool) -> TestResult {
    use base64::Engine as _;
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        let workspace = files(&[("a.txt", "real a\n"), ("b.txt", "real b\n")]);
        message(f, &cookie, conversation.id.as_str(), "Go").await?;
        assert!(matches!(
            link::serve(&mut runtime, &workspace).await?,
            WorkspaceOp::Manifest { .. }
        ));
        let (asked, op) = link::request(&mut runtime).await?;
        let WorkspaceOp::Blobs { hashes } = op else {
            return Err("expected blobs".into());
        };
        // a.txt's name with other bytes; b.txt's own bytes.
        let part = |hash: &str, bytes: &[u8]| {
            json!({"kind": "blob_part", "hash": hash, "offset": 0,
                "data": base64::engine::general_purpose::STANDARD.encode(bytes), "complete": true})
        };
        link::reply(
            &mut runtime,
            asked,
            &[part(&hashes[0], b"forged\n"), part(&hashes[1], b"real b\n")],
        )
        .await?;
        assert!(matches!(
            link::next_action(&mut runtime).await?,
            ClientFrame::UserMessage { .. }
        ));
        let workspace_id: uuid::Uuid =
            sqlx::query_scalar("SELECT workspace_id FROM conversations WHERE id=$1")
                .bind(uuid::Uuid::parse_str(conversation.id.as_str())?)
                .fetch_one(&f.pool)
                .await?;
        let blobs = f
            .config_storage_root()
            .join("changes")
            .join(workspace_id.to_string())
            .join("blobs");
        let mut stored: Vec<String> = std::fs::read_dir(&blobs)?
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect();
        stored.sort();
        assert_eq!(
            stored,
            [link::sha(b"real b\n")],
            "neither the forged bytes nor a.txt's name"
        );
        Ok(())
    })
    .await
}
