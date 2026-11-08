//! GW-I-045 and GW-I-047: a task's workspace files and uploads over HTTP - storage under `uploads/`,
//! limits refused before storing, paths that climb refused, and access only by workspace ownership or share (the
//! administrator role grants no content). The walker's link and race cases are unit tests in `src/files/`.
//! GW-I-013's caps: the tree page size and paging, the preview cut and the download limit.
#[path = "support/files_api.rs"]
mod files_api;
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};

use files_api::{Result, body, call, code, task, task_with, upload};

fn files(f: &Fixture, cookie: &str, task: &str, path: &str) -> reqwest::RequestBuilder {
    call(
        f,
        cookie,
        Method::GET,
        &format!("/api/v1/conversations/{task}/files"),
    )
    .query(&[("path", path)])
}

fn content(f: &Fixture, cookie: &str, task: &str, path: &str) -> reqwest::RequestBuilder {
    call(
        f,
        cookie,
        Method::GET,
        &format!("/api/v1/conversations/{task}/files/content"),
    )
    .query(&[("path", path)])
}

// TEST: GW-I-013
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_045_uploads_land_in_uploads_under_a_sanitised_name_and_never_overwrite(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie).await?;
        let (status, first) =
            body(upload(f, &cookie, &id, "My notes (1).txt", b"hello".to_vec()).await?).await?;
        assert_eq!(status, 201, "{first}");
        assert_eq!(
            first,
            json!({"path":"uploads/My_notes__1_.txt","name":"My_notes__1_.txt","size":5})
        );
        let (_, second) =
            body(upload(f, &cookie, &id, "../../My notes (1).txt", b"again".to_vec()).await?)
                .await?;
        assert_eq!(second["path"], "uploads/My_notes__1_-2.txt");

        let (status, root) = body(files(f, &cookie, &id, "").send().await?).await?;
        assert_eq!(status, 200, "{root}");
        assert_eq!(root["items"][0]["name"], "uploads");
        assert_eq!(root["items"][0]["kind"], "dir");
        let (_, listed) = body(files(f, &cookie, &id, "uploads").send().await?).await?;
        let names: Vec<&str> = listed["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .filter_map(|item| item["name"].as_str())
            .collect();
        assert_eq!(names, ["My_notes__1_-2.txt", "My_notes__1_.txt"]);
        assert_eq!(listed["next_after"], Value::Null);

        let response = content(f, &cookie, &id, "uploads/My_notes__1_.txt")
            .send()
            .await?;
        let tag = response
            .headers()
            .get("etag")
            .ok_or("etag")?
            .to_str()?
            .to_owned();
        let (status, preview) = body(response).await?;
        assert_eq!(status, 200);
        assert_eq!(
            (
                preview["text"].as_str(),
                preview["binary"].as_bool(),
                preview["size"].as_u64()
            ),
            (Some("hello"), Some(false), Some(5))
        );
        let unchanged = content(f, &cookie, &id, "uploads/My_notes__1_.txt")
            .header("if-none-match", &tag)
            .send()
            .await?;
        assert_eq!(unchanged.status(), 304);

        let download = call(
            f,
            &cookie,
            Method::GET,
            &format!("/api/v1/conversations/{id}/files/download"),
        )
        .query(&[("path", "uploads/My_notes__1_-2.txt")])
        .send()
        .await?;
        assert_eq!(download.status(), 200);
        let headers = download.headers().clone();
        assert_eq!(headers["content-type"], "application/octet-stream");
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(
            headers["content-disposition"],
            "attachment; filename=\"My_notes__1_-2.txt\""
        );
        assert_eq!(download.bytes().await?.as_ref(), b"again");

        let (_, binary) =
            body(upload(f, &cookie, &id, "data.bin", vec![0, 159, 146, 150]).await?).await?;
        let (_, preview) = body(
            content(f, &cookie, &id, binary["path"].as_str().ok_or("path")?)
                .send()
                .await?,
        )
        .await?;
        assert_eq!(
            (preview["binary"].as_bool(), preview["text"].is_null()),
            (Some(true), true)
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-013
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_045_paths_that_climb_or_reach_other_tasks_notes_are_refused(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie).await?;
        for climbing in ["../other", "/etc/passwd", "uploads/../../x", "a//b"] {
            assert_eq!(
                code(files(f, &cookie, &id, climbing).send().await?).await?,
                (400, "path_invalid".into()),
                "{climbing}"
            );
            assert_eq!(
                code(content(f, &cookie, &id, climbing).send().await?).await?,
                (400, "path_invalid".into()),
                "{climbing}"
            );
        }
        assert_eq!(
            code(
                content(f, &cookie, &id, ".tqa/other/notes.md")
                    .send()
                    .await?
            )
            .await?
            .0,
            404
        );
        assert_eq!(
            code(content(f, &cookie, &id, "missing.txt").send().await?)
                .await?
                .0,
            404
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-013
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_045_limits_are_checked_before_anything_is_stored(pool: PgPool) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[files]\nupload_bytes=1024\nuploads_bytes=2048",
        async |f| {
            let (_, cookie) = f.admin().await?;
            let id = task(f, &cookie).await?;
            assert_eq!(
                code(upload(f, &cookie, &id, "big.bin", vec![1; 1025]).await?).await?,
                (413, "upload_too_large".into())
            );
            assert_eq!(
                code(files(f, &cookie, &id, "uploads").send().await?)
                    .await?
                    .0,
                404,
                "nothing was created"
            );
            for name in ["a.bin", "b.bin"] {
                assert_eq!(
                    upload(f, &cookie, &id, name, vec![1; 1000]).await?.status(),
                    201
                );
            }
            assert_eq!(
                code(upload(f, &cookie, &id, "c.bin", vec![1; 100]).await?).await?,
                (409, "uploads_full".into())
            );
            let (_, listed) = body(files(f, &cookie, &id, "uploads").send().await?).await?;
            assert_eq!(listed["items"].as_array().map(Vec::len), Some(2));
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-013
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_045_access_is_workspace_access_and_administrators_get_no_content(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (_, owner) = f.member("owner@example.invalid").await?;
        let (reader, reader_cookie) = f.member("reader@example.invalid").await?;
        let (writer, writer_cookie) = f.member("writer@example.invalid").await?;
        let id = task(f, &owner).await?;
        assert_eq!(
            upload(f, &owner, &id, "plan.md", b"# Plan".to_vec())
                .await?
                .status(),
            201
        );
        // The administrator role grants metadata only, never files (answers-066 Q7).
        for cookie in [&admin, &reader_cookie] {
            assert_eq!(code(files(f, cookie, &id, "").send().await?).await?.0, 403);
            assert_eq!(
                code(upload(f, cookie, &id, "x.txt", b"x".to_vec()).await?)
                    .await?
                    .0,
                403
            );
        }
        let (_, conversation) = body(
            call(
                f,
                &owner,
                Method::GET,
                &format!("/api/v1/conversations/{id}"),
            )
            .send()
            .await?,
        )
        .await?;
        let workspace = conversation["workspace_id"]
            .as_str()
            .ok_or("workspace id")?
            .to_owned();
        for (user, access) in [(reader.id, "read"), (writer.id, "write")] {
            let shared = call(
                f,
                &owner,
                Method::PUT,
                &format!("/api/v1/workspaces/{workspace}/shares/{user}"),
            )
            .json(&json!({"access":access}))
            .send()
            .await?;
            assert_eq!(shared.status(), 200);
        }
        // A read share lists and reads but cannot upload; a write share can.
        assert_eq!(
            files(f, &reader_cookie, &id, "uploads")
                .send()
                .await?
                .status(),
            200
        );
        assert_eq!(
            content(f, &reader_cookie, &id, "uploads/plan.md")
                .send()
                .await?
                .status(),
            200
        );
        assert_eq!(
            code(upload(f, &reader_cookie, &id, "x.txt", b"x".to_vec()).await?).await?,
            (403, "forbidden".into())
        );
        assert_eq!(
            upload(f, &writer_cookie, &id, "y.txt", b"y".to_vec())
                .await?
                .status(),
            201
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-013
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_045_programs_upload_with_a_token_and_cookie_uploads_need_the_origin(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie).await?;
        let no_origin = f
            .client
            .post(f.url(&format!("/api/v1/conversations/{id}/uploads")))
            .query(&[("name", "x.txt")])
            .header("cookie", &cookie)
            .body("x")
            .send()
            .await?;
        assert_eq!(no_origin.status(), 403);
        let (_, created) = body(
            call(f, &cookie, Method::POST, "/api/v1/tokens")
                .json(&json!({"name":"uploader","expires_in_days":1}))
                .send()
                .await?,
        )
        .await?;
        let token = created["token"].as_str().ok_or("token")?;
        let program = f
            .client
            .post(f.url(&format!("/api/v1/conversations/{id}/uploads")))
            .query(&[("name", "from-ci.log")])
            .bearer_auth(token)
            .body("ok")
            .send()
            .await?;
        assert_eq!(program.status(), 201);
        let listed = f
            .client
            .get(f.url(&format!("/api/v1/conversations/{id}/files")))
            .query(&[("path", "uploads")])
            .bearer_auth(token)
            .send()
            .await?;
        assert_eq!(listed.status(), 200);
        Ok(())
    })
    .await
}

// TEST: GW-I-013
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_045_a_message_attaches_only_files_that_were_uploaded(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie).await?;
        let path = format!("/api/v1/conversations/{id}/messages");
        // A file outside the task's uploads is refused before the message is stored.
        let missing = f
            .action(
                &path,
                &cookie,
                Some("missing"),
                &json!({"text":"Read it","attachments":["uploads/missing.txt"]}),
            )
            .await?;
        assert_eq!(code(missing).await?, (400, "attachment_missing".into()));
        let (_, stored) =
            body(upload(f, &cookie, &id, "notes.txt", b"hello".to_vec()).await?).await?;
        let reference = stored["path"].as_str().ok_or("path")?.to_owned();
        let accepted = f
            .action(
                &path,
                &cookie,
                Some("present"),
                &json!({"text":"Read it","attachments":[reference]}),
            )
            .await?;
        assert_eq!(accepted.status(), 202);
        // A mixed list is refused as a whole.
        let mixed = f
            .action(
                &path,
                &cookie,
                Some("mixed"),
                &json!({"text":"Both","attachments":[reference, "uploads/gone.txt"]}),
            )
            .await?;
        assert_eq!(code(mixed).await?, (400, "attachment_missing".into()));
        // The protocol rule still answers a reference outside uploads/ first.
        let outside = f
            .action(
                &path,
                &cookie,
                Some("outside"),
                &json!({"text":"No","attachments":["notes.txt"]}),
            )
            .await?;
        assert_eq!(outside.status(), 400);
        Ok(())
    })
    .await
}

// GW-I-048: a run's changes are the difference between the workspace snapshot taken when its message was
// delivered and the next one; the latest run is compared with the live workspace. The fake runtime plays the agent:
// it reports the run lifecycle while the test writes to the mounted workspace.
fn run_event(sequence: u64, run: &str, event: tqa_types::Event) -> Result<tqa_types::ServerFrame> {
    Ok(tqa_types::ServerFrame::Event {
        seq: sequence.into(),
        run_id: Some(run.parse()?),
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    })
}

async fn run_lifecycle(
    runtime: &mut support::runtime::Runtime,
    first: u64,
    run: &str,
    work: impl FnOnce() -> std::io::Result<()>,
) -> TestResult {
    let run_id: tqa_types::RunId = run.parse()?;
    runtime
        .send(&run_event(
            first,
            run,
            tqa_types::Event::RunStarted {
                run_id: run_id.clone(),
            },
        )?)
        .await?;
    work()?;
    runtime
        .send(&run_event(
            first + 1,
            run,
            tqa_types::Event::RunCompleted { run_id },
        )?)
        .await?;
    Ok(())
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

fn changes(f: &Fixture, cookie: &str, id: &str, run: &str) -> reqwest::RequestBuilder {
    call(
        f,
        cookie,
        Method::GET,
        &format!("/api/v1/conversations/{id}/runs/{run}/changes"),
    )
}

// TEST: GW-I-013
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_048_a_runs_changes_come_from_the_snapshots_around_it(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = conversation.id.to_string();
        let mut runtime = f.runtime(&conversation).await?;
        let (mount, _) = f.backend.mounts(&conversation.id).ok_or("mounts")?;
        let tqa_sandbox::WorkspaceSource::HostDirectory(root) = mount.source else {
            return Err("a host-directory workspace".into());
        };
        std::fs::write(root.join("notes.txt"), "one\n")?;
        // A run reported before any message was delivered has no snapshot to compare with.
        run_lifecycle(&mut runtime, 1, "r-0", || Ok(())).await?;
        f.until_sequence(&conversation.id, 2).await?;
        assert_eq!(
            code(changes(f, &cookie, &id, "r-0").send().await?).await?,
            (404, "changes_unavailable".to_owned())
        );

        message(f, &cookie, &id, "Write the report").await?;
        assert!(matches!(
            runtime.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        run_lifecycle(&mut runtime, 3, "r-1", || {
            std::fs::write(root.join("notes.txt"), "one\ntwo\n")?;
            std::fs::write(root.join("report.md"), "# Report\n")?;
            std::fs::create_dir_all(root.join("uploads"))?;
            std::fs::write(root.join("uploads/in.csv"), "a,b\n")
        })
        .await?;
        f.until_sequence(&conversation.id, 4).await?;
        let (status, latest) = body(changes(f, &cookie, &id, "r-1").send().await?).await?;
        assert_eq!(status, 200, "{latest}");
        let expected = json!([
            {"path":"notes.txt","status":"modified","added":1,"removed":0,"binary":false,"too_large":false},
            {"path":"report.md","status":"added","added":1,"removed":0,"binary":false,"too_large":false},
        ]);
        assert_eq!(latest["files"], expected);
        assert_eq!((latest["live"].clone(), latest["running"].clone()), (json!(true), json!(false)));
        let diff_of = |path: &str| {
            call(
                f,
                &cookie,
                Method::GET,
                &format!("/api/v1/conversations/{id}/runs/r-1/changes/diff"),
            )
            .query(&[("path", path.to_owned())])
        };
        let (status, diff) = body(diff_of("notes.txt").send().await?).await?;
        assert_eq!(status, 200, "{diff}");
        assert_eq!(
            diff["unified"],
            "--- a/notes.txt\n+++ b/notes.txt\n@@ -1 +1,2 @@\n one\n+two\n"
        );
        assert_eq!(code(diff_of("../notes.txt").send().await?).await?.0, 400);
        assert_eq!(code(diff_of("missing.txt").send().await?).await?.0, 404);

        // The next message's snapshot closes run r-1; r-2 then removes the report.
        message(f, &cookie, &id, "Remove the report").await?;
        assert!(matches!(
            runtime.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        run_lifecycle(&mut runtime, 5, "r-2", || {
            std::fs::remove_file(root.join("report.md"))
        })
        .await?;
        f.until_sequence(&conversation.id, 6).await?;
        let (_, closed) = body(changes(f, &cookie, &id, "r-1").send().await?).await?;
        assert_eq!((closed["files"].clone(), closed["live"].clone()), (expected, json!(false)));
        let (_, removed) = body(changes(f, &cookie, &id, "r-2").send().await?).await?;
        assert_eq!(
            removed["files"],
            json!([{"path":"report.md","status":"deleted","added":0,"removed":1,"binary":false,"too_large":false}])
        );
        assert_eq!(code(changes(f, &cookie, &id, "r-9").send().await?).await?.0, 404);
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        assert_ne!(
            code(changes(f, &stranger, &id, "r-1").send().await?).await?.0,
            200
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-048
// The person uploads a file and asks the run to fix it: the run's edit is its change. A file the person uploads after
// the run ends (for the next message) is theirs, whether the run is compared with the live workspace or with the next
// message's snapshot.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_048_an_edited_upload_is_the_runs_change_and_a_later_upload_is_not(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = conversation.id.to_string();
        let mut runtime = f.runtime(&conversation).await?;
        let (mount, _) = f.backend.mounts(&conversation.id).ok_or("mounts")?;
        let tqa_sandbox::WorkspaceSource::HostDirectory(root) = mount.source else {
            return Err("a host-directory workspace".into());
        };
        let uploaded = upload(f, &cookie, &id, "prices.py", b"PRICE = 1\n".to_vec()).await?;
        assert_eq!(uploaded.status(), 201);
        message(f, &cookie, &id, "Fix prices.py").await?;
        assert!(matches!(
            runtime.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        run_lifecycle(&mut runtime, 1, "r-1", || {
            std::fs::write(root.join("uploads/prices.py"), "PRICE = 2\n")
        })
        .await?;
        f.until_sequence(&conversation.id, 2).await?;
        let later = upload(f, &cookie, &id, "later.csv", b"a,b\n".to_vec()).await?;
        assert_eq!(later.status(), 201);
        let expected = json!([
            {"path":"uploads/prices.py","status":"modified","added":1,"removed":1,"binary":false,"too_large":false},
        ]);
        let (status, live) = body(changes(f, &cookie, &id, "r-1").send().await?).await?;
        assert_eq!(status, 200, "{live}");
        assert_eq!((live["files"].clone(), live["live"].clone()), (expected.clone(), json!(true)));
        // The next message's snapshot, taken with later.csv in place, closes r-1 the same.
        message(f, &cookie, &id, "Use later.csv too").await?;
        assert!(matches!(
            runtime.action().await?,
            tqa_types::ClientFrame::UserMessage { .. }
        ));
        let (_, closed) = body(changes(f, &cookie, &id, "r-1").send().await?).await?;
        assert_eq!((closed["files"].clone(), closed["live"].clone()), (expected, json!(false)));
        Ok(())
    })
    .await
}

// TEST: GW-I-013
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_013_the_tree_page_is_1_through_500_and_pages_with_after(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task_with(
            f,
            &cookie,
            &[("a.txt", vec![1]), ("b.txt", vec![1]), ("c.txt", vec![1])],
        )
        .await?;
        let page = |limit: &str, after: Option<&str>| {
            let mut request = files(f, &cookie, &id, "uploads").query(&[("limit", limit)]);
            if let Some(after) = after {
                request = request.query(&[("after", after)]);
            }
            request
        };
        for limit in ["0", "501"] {
            assert_eq!(
                code(page(limit, None).send().await?).await?,
                (400, "invalid_request".into()),
                "limit {limit}"
            );
        }
        assert_eq!(page("500", None).send().await?.status(), 200);
        let (_, first) = body(page("2", None).send().await?).await?;
        let names = |value: &Value| -> Vec<String> {
            value["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item["name"].as_str().map(str::to_owned))
                .collect()
        };
        assert_eq!(names(&first), ["a.txt", "b.txt"]);
        assert_eq!(first["next_after"], "b.txt");
        let (_, second) = body(page("2", Some("b.txt")).send().await?).await?;
        assert_eq!(names(&second), ["c.txt"]);
        assert_eq!(second["next_after"], Value::Null, "the last page");
        Ok(())
    })
    .await
}

// TEST: GW-I-013
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_013_a_preview_is_cut_at_preview_bytes_and_marked_truncated(
    pool: PgPool,
) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[files]\npreview_bytes=1024\ndownload_bytes=2048",
        async |f| {
            let (_, cookie) = f.admin().await?;
            let id = task_with(
                f,
                &cookie,
                &[
                    ("long.txt", vec![b'x'; 1500]),
                    ("fits.txt", vec![b'y'; 1024]),
                ],
            )
            .await?;
            let (status, long) =
                body(content(f, &cookie, &id, "uploads/long.txt").send().await?).await?;
            assert_eq!(status, 200);
            assert_eq!(long["size"], 1500);
            assert_eq!(
                long["text"].as_str().map(str::len),
                Some(1024),
                "cut at preview_bytes"
            );
            assert_eq!(long["truncated"], true);
            let (_, fits) =
                body(content(f, &cookie, &id, "uploads/fits.txt").send().await?).await?;
            assert_eq!(fits["text"].as_str().map(str::len), Some(1024));
            assert_eq!(fits["truncated"], false, "exactly preview_bytes is whole");
            Ok(())
        },
    )
    .await
}

// TEST: GW-I-013
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_013_a_download_above_download_bytes_is_413(pool: PgPool) -> TestResult {
    support::run_configured(
        pool,
        None,
        "[files]\npreview_bytes=1024\ndownload_bytes=2048",
        async |f| {
            let (_, cookie) = f.admin().await?;
            let id = task_with(
                f,
                &cookie,
                &[("big.bin", vec![7; 2049]), ("edge.bin", vec![7; 2048])],
            )
            .await?;
            let download = |name: &str| {
                call(
                    f,
                    &cookie,
                    Method::GET,
                    &format!("/api/v1/conversations/{id}/files/download"),
                )
                .query(&[("path", format!("uploads/{name}"))])
            };
            assert_eq!(
                code(download("big.bin").send().await?).await?,
                (413, "body_too_large".into())
            );
            let edge = download("edge.bin").send().await?;
            assert_eq!(edge.status(), 200, "exactly download_bytes is served");
            assert_eq!(edge.bytes().await?.len(), 2048);
            Ok(())
        },
    )
    .await
}
