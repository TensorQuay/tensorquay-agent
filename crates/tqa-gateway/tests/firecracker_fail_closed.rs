//! GW-I-057: on a microVM install the gateway never reads a workspace on the host. Files are read through the
//! workspace's runtime (ADR-0030): listing, preview, download, uploads (GW-I-083), run changes (GW-I-084) and the
//! attachment check answer as on Docker; readiness reports the configured backend. Fork copies the image instead (GW-I-064).

#[path = "support/fixture.rs"]
mod support;

use serde_json::json;
use sqlx::PgPool;
use support::{Fixture, TestResult, workspace_link as link};
use tqa_types::WorkspaceOp;

fn firecracker() -> String {
    format!(
        "[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\n",
        "b".repeat(64)
    )
}

async fn request(
    f: &Fixture,
    method: reqwest::Method,
    path: &str,
    cookie: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    f.client
        .request(method, f.url(path))
        .header("origin", f.origin())
        .header("cookie", cookie)
        .json(&serde_json::json!({ "name": "copy" }))
        .send()
        .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_057_files_are_read_through_the_runtime_on_the_microvm_backend(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &firecracker(), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        // A listing: the runtime's page, in the Docker view's shape.
        let pending = link::get(f, &format!("/api/v1/conversations/{id}/files"), &cookie);
        let (asked, op) = link::request(&mut runtime).await?;
        let listed = WorkspaceOp::List { path: String::new(), after: None, limit: 200 };
        assert_eq!(op, listed);
        let items = [link::item("src", "dir", 4096), link::item("notes.txt", "file", 5)];
        link::reply(&mut runtime, asked, &[json!({"kind": "listing", "items": items})]).await?;
        let page = pending.await??;
        assert_eq!(page.status, 200);
        assert_eq!(
            page.json()?,
            json!({"path": "", "next_after": null, "items": [
                {"name": "src", "kind": "dir", "size": 0, "modified_at": "2023-11-14T22:13:20Z"},
                {"name": "notes.txt", "kind": "file", "size": 5, "modified_at": "2023-11-14T22:13:20Z"},
            ]})
        );
        // A preview: the text, and the same validator a directory would give.
        let pending = link::get(f, &format!("/api/v1/conversations/{id}/files/content?path=notes.txt"), &cookie);
        let (asked, op) = link::request(&mut runtime).await?;
        let read = WorkspaceOp::Read { path: "notes.txt".into(), offset: 0, max_bytes: 131_072 };
        assert_eq!(op, read);
        link::reply(&mut runtime, asked, &[link::content("aGVsbG8=", 5, true)]).await?;
        let preview = pending.await??;
        let etag = "\"5-6553f100-2a\"";
        assert_eq!(preview.headers.get("etag").and_then(|value| value.to_str().ok()), Some(etag));
        let body = preview.json()?;
        assert_eq!((&body["text"], &body["etag"], &body["binary"], &body["truncated"]), (&json!("hello"), &json!(etag), &json!(false), &json!(false)));
        // A download: the bytes as an attachment.
        let pending = link::get(f, &format!("/api/v1/conversations/{id}/files/download?path=notes.txt"), &cookie);
        let (asked, op) = link::request(&mut runtime).await?;
        assert_eq!(op, read);
        link::reply(&mut runtime, asked, &[link::content("aGVsbG8=", 5, true)]).await?;
        let download = pending.await??;
        assert_eq!((download.status, download.body.as_slice()), (200, b"hello".as_slice()));
        // A message's attachments are checked by the runtime too.
        let messages = format!("/api/v1/conversations/{id}/messages");
        let attach = json!({"text": "Read it", "attachments": ["uploads/a.txt"]});
        let (sent, answered) = tokio::join!(f.action(&messages, &cookie, Some("attached"), &attach), async {
            let (asked, op) = link::request(&mut runtime).await?;
            assert_eq!(op, WorkspaceOp::Stat { paths: vec!["uploads/a.txt".into()] });
            let stats = json!({"kind": "stats", "items": [{"exists": true, "kind": "file", "size": 3}]});
            link::reply(&mut runtime, asked, &[stats]).await
        });
        answered?;
        assert_eq!(sent?.status(), 202);
        // Run changes are read through the runtime too (GW-I-084): an unknown run is not found, as on Docker.
        let run = uuid::Uuid::now_v7();
        let get = reqwest::Method::GET;
        let unknown = request(f, get.clone(), &format!("/api/v1/conversations/{id}/runs/{run}/changes"), &cookie).await?;
        assert_eq!(unknown.status(), 404);
        let ready: serde_json::Value = request(f, get, "/readyz?detail=1", &cookie).await?.json().await?;
        assert_eq!(ready["sandbox"]["backend"], "firecracker", "{ready}");
        Ok(())
    })
    .await
}

fn encoded(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_057_large_files_are_read_in_chunks_and_odd_answers_are_refused(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &firecracker(), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let chunk = tqa_types::WORKSPACE_CHUNK_BYTES;
        let size = u64::try_from(chunk + 3)?;
        let first = link::content(&encoded(&vec![b'a'; chunk]), size, false);
        // Links and other kinds keep their names; an `after` that names no entry is an empty page, never a request.
        let pending = link::get(f, &format!("/api/v1/conversations/{id}/files"), &cookie);
        let (asked, _) = link::request(&mut runtime).await?;
        let items = [
            link::item("link", "link", 7),
            link::item("pipe", "other", 0),
        ];
        link::reply(
            &mut runtime,
            asked,
            &[json!({"kind": "listing", "items": items})],
        )
        .await?;
        let page = pending.await??.json()?;
        assert_eq!(
            (
                &page["items"][0]["kind"],
                &page["items"][1]["kind"],
                &page["items"][0]["size"]
            ),
            (&json!("link"), &json!("other"), &json!(0))
        );
        let odd = link::get(
            f,
            &format!("/api/v1/conversations/{id}/files?after=a%2Fb"),
            &cookie,
        )
        .await??;
        assert_eq!(
            odd.json()?,
            json!({"path": "", "items": [], "next_after": null})
        );
        // A preview past one chunk reads on from where the first stopped.
        let pending = link::get(
            f,
            &format!("/api/v1/conversations/{id}/files/content?path=big.txt"),
            &cookie,
        );
        let (asked, _) = link::request(&mut runtime).await?;
        link::reply(&mut runtime, asked, std::slice::from_ref(&first)).await?;
        let (asked, op) = link::request(&mut runtime).await?;
        let offset = u64::try_from(chunk)?;
        assert_eq!(
            op,
            WorkspaceOp::Read {
                path: "big.txt".into(),
                offset,
                max_bytes: 131_072
            }
        );
        link::reply(
            &mut runtime,
            asked,
            &[link::content(&encoded(b"end"), size, true)],
        )
        .await?;
        let preview = pending.await??.json()?;
        assert_eq!(preview["text"].as_str().map(str::len), Some(chunk + 3));
        // A file that changes between chunks is refused rather than stitched together: a race, so retryable.
        let pending = link::get(
            f,
            &format!("/api/v1/conversations/{id}/files/content?path=big.txt"),
            &cookie,
        );
        let (asked, _) = link::request(&mut runtime).await?;
        link::reply(&mut runtime, asked, std::slice::from_ref(&first)).await?;
        let (asked, _) = link::request(&mut runtime).await?;
        link::reply(
            &mut runtime,
            asked,
            &[link::content(&encoded(b"endless"), size + 4, true)],
        )
        .await?;
        let changed = pending.await??.json()?;
        assert_eq!(
            (&changed["error"]["code"], &changed["error"]["retryable"]),
            (&json!("file_changed"), &json!(true)),
            "{changed}"
        );
        // A download streams every chunk; one over the limit is refused before any byte.
        let pending = link::get(
            f,
            &format!("/api/v1/conversations/{id}/files/download?path=big.txt"),
            &cookie,
        );
        let (asked, _) = link::request(&mut runtime).await?;
        link::reply(&mut runtime, asked, std::slice::from_ref(&first)).await?;
        let (asked, _) = link::request(&mut runtime).await?;
        link::reply(
            &mut runtime,
            asked,
            &[link::content(&encoded(b"end"), size, true)],
        )
        .await?;
        let download = pending.await??;
        assert_eq!((download.status, download.body.len()), (200, chunk + 3));
        let pending = link::get(
            f,
            &format!("/api/v1/conversations/{id}/files/download?path=huge.bin"),
            &cookie,
        );
        let (asked, _) = link::request(&mut runtime).await?;
        let huge = link::content(&encoded(&vec![0; chunk]), 200 * 1024 * 1024, false);
        link::reply(&mut runtime, asked, &[huge]).await?;
        assert_eq!(pending.await??.status, 413);
        // An attachment the runtime does not find is refused.
        let messages = format!("/api/v1/conversations/{id}/messages");
        let attach = json!({"text": "Read it", "attachments": ["uploads/gone.txt"]});
        let (sent, answered) =
            tokio::join!(f.action(&messages, &cookie, Some("gone"), &attach), async {
                let (asked, _) = link::request(&mut runtime).await?;
                link::reply(
                    &mut runtime,
                    asked,
                    &[json!({"kind": "stats", "items": [{"exists": false}]})],
                )
                .await
            });
        answered?;
        assert_eq!(sent?.status(), 400);
        Ok(())
    })
    .await
}
