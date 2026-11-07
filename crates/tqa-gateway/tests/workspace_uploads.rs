//! GW-I-083 (ADR-0030): uploads into a block-image workspace go through its runtime, chunk by chunk, with the Docker
//! limits and names; the attachment check can start the owner's sandbox, outside the action's transaction.
#[path = "support/fixture.rs"]
mod support;

use std::time::Duration;

use base64::Engine as _;
use futures::StreamExt as _;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, runtime::Runtime, workspace_link as link};
use tqa_types::{WORKSPACE_CHUNK_BYTES, WorkspaceOp};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn firecracker(extra: &str) -> String {
    format!(
        "[firecracker]\nvmm_image='sha256:{}'\ngateway_address='192.0.2.10'\n{extra}",
        "b".repeat(64)
    )
}

/// Starts an upload of `body` the fake runtime answers meanwhile.
fn upload(
    f: &Fixture,
    cookie: &str,
    id: &tqa_types::ConversationId,
    name: &str,
    body: reqwest::Body,
) -> tokio::task::JoinHandle<std::result::Result<link::Answer, reqwest::Error>> {
    let request = f
        .client
        .post(f.url(&format!("/api/v1/conversations/{id}/uploads")))
        .query(&[("name", name)])
        .header("origin", f.origin())
        .header("cookie", cookie)
        .body(body);
    tokio::spawn(async move {
        let response = request.send().await?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.bytes().await?.to_vec();
        Ok(link::Answer {
            status,
            headers,
            body,
        })
    })
}

/// Answers the usage listing of `uploads/` with `used` bytes in one file.
async fn usage(runtime: &mut Runtime, used: u64) -> Result<()> {
    let (asked, op) = link::request(runtime).await?;
    let WorkspaceOp::List { path, .. } = op else {
        return Err(format!("expected the uploads listing, got {op:?}").into());
    };
    assert_eq!(path, "uploads");
    let items = [link::item("old.bin", "file", used)];
    link::reply(
        runtime,
        asked,
        &[json!({"kind": "listing", "items": items})],
    )
    .await?;
    Ok(())
}

/// The next upload chunk: its id, name, offset, bytes and whether it is the last.
async fn chunk(runtime: &mut Runtime) -> Result<(u64, u64, String, u64, Vec<u8>, bool)> {
    let (asked, op) = link::request(runtime).await?;
    let WorkspaceOp::Upload {
        upload_id,
        name,
        offset,
        data,
        last,
    } = op
    else {
        return Err(format!("expected an upload chunk, got {op:?}").into());
    };
    let data = base64::engine::general_purpose::STANDARD.decode(data)?;
    Ok((asked, upload_id, name, offset, data, last))
}

fn received(size: u64, stored: Option<&str>) -> Value {
    let mut result = json!({"kind": "uploaded", "received": size});
    if let Some(name) = stored {
        result["file"] = json!({"path": format!("uploads/{name}"), "name": name, "size": size});
    }
    result
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_083_an_upload_streams_to_the_runtime_in_order(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let body = vec![b'x'; WORKSPACE_CHUNK_BYTES + 3];
        let pending = upload(f, &cookie, id, "My Report.pdf", body.clone().into());
        usage(&mut runtime, 10).await?;
        let (asked, upload_id, name, offset, data, last) = chunk(&mut runtime).await?;
        assert_eq!(
            (name.as_str(), offset, data.len(), last),
            ("My_Report.pdf", 0, WORKSPACE_CHUNK_BYTES, false)
        );
        link::reply(
            &mut runtime,
            asked,
            &[received(offset + data.len() as u64, None)],
        )
        .await?;
        let (asked, same, _, offset, data, last) = chunk(&mut runtime).await?;
        assert_eq!(
            (same, offset, data.len(), last),
            (upload_id, WORKSPACE_CHUNK_BYTES as u64, 3, true)
        );
        let size = body.len() as u64;
        link::reply(
            &mut runtime,
            asked,
            &[received(size, Some("My_Report-2.pdf"))],
        )
        .await?;
        let answer = pending.await??;
        assert_eq!(answer.status, 201);
        assert_eq!(
            answer.json()?,
            json!({"path": "uploads/My_Report-2.pdf", "name": "My_Report-2.pdf", "size": size})
        );
        // An empty file is one empty last chunk, with a fresh upload id.
        let pending = upload(f, &cookie, id, "empty.txt", Vec::new().into());
        usage(&mut runtime, 0).await?;
        let (asked, other, _, offset, data, last) = chunk(&mut runtime).await?;
        assert!(other != upload_id && offset == 0 && data.is_empty() && last);
        link::reply(&mut runtime, asked, &[received(0, Some("empty.txt"))]).await?;
        assert_eq!(pending.await??.status, 201);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_083_limits_and_access_are_those_of_docker(pool: PgPool) -> TestResult {
    let config = firecracker("[files]\nupload_bytes=1024\nuploads_bytes=2048\n");
    support::run_configured(pool, None, &config, async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let id = &conversation.id;
        let mut runtime = f.runtime(&conversation).await?;
        let code = |answer: &link::Answer| -> Result<(u16, Value)> {
            Ok((answer.status, answer.json()?["error"]["code"].clone()))
        };
        // Declared over the per-file limit: refused before the runtime hears of it.
        let over = upload(f, &cookie, id, "big.bin", vec![0; 2000].into()).await??;
        assert_eq!(code(&over)?, (413, json!("upload_too_large")));
        // Over the workspace's upload space counted from the listing: refused before any chunk.
        let pending = upload(f, &cookie, id, "a.bin", vec![0; 100].into());
        usage(&mut runtime, 2000).await?;
        assert_eq!(code(&pending.await??)?, (409, json!("uploads_full")));
        // A listing past its bound is refused rather than undercounted.
        let pending = upload(f, &cookie, id, "a.bin", vec![0; 1].into());
        for page in 0..20 {
            let (asked, _) = link::request(&mut runtime).await?;
            let items = [link::item(&format!("f{page:02}"), "file", 0)];
            link::reply(
                &mut runtime,
                asked,
                &[json!({"kind": "listing", "items": items, "next_after": format!("f{page:02}")})],
            )
            .await?;
        }
        assert_eq!(code(&pending.await??)?, (409, json!("uploads_full")));
        // A body over the limit without a declared length stops at the limit.
        let streamed = reqwest::Body::wrap_stream(futures::stream::iter(
            [vec![0_u8; 700], vec![0_u8; 700]].map(Ok::<_, std::io::Error>),
        ));
        let pending = upload(f, &cookie, id, "b.bin", streamed);
        usage(&mut runtime, 0).await?;
        assert_eq!(code(&pending.await??)?, (413, json!("upload_too_large")));
        // A read share cannot upload.
        let (reader, reader_cookie) = f.member("reader@example.invalid").await?;
        let workspace: uuid::Uuid =
            sqlx::query_scalar("SELECT workspace_id FROM conversations WHERE id=$1")
                .bind(uuid::Uuid::parse_str(id.as_str())?)
                .fetch_one(&f.pool)
                .await?;
        let shared = f
            .client
            .put(f.url(&format!(
                "/api/v1/workspaces/{workspace}/shares/{}",
                reader.id
            )))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .json(&json!({"access": "read"}))
            .send()
            .await?;
        assert_eq!(shared.status(), 200);
        let refused = upload(f, &reader_cookie, id, "c.bin", vec![0; 1].into()).await??;
        assert_eq!(code(&refused)?.0, 403);
        assert!(
            runtime
                .try_action(Duration::from_millis(200))
                .await?
                .is_none(),
            "the runtime heard nothing more"
        );
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_083_a_client_that_leaves_mid_upload_stops_it(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let mut runtime = f.runtime(&conversation).await?;
        // Two full chunks, then the body stalls: the first chunk is sent, the second waits for the rest.
        let chunks = futures::stream::iter(
            [
                vec![1_u8; WORKSPACE_CHUNK_BYTES],
                vec![2_u8; WORKSPACE_CHUNK_BYTES],
            ]
            .map(Ok::<_, std::io::Error>),
        )
        .chain(futures::stream::pending());
        let pending = upload(
            f,
            &cookie,
            &conversation.id,
            "slow.bin",
            reqwest::Body::wrap_stream(chunks),
        );
        usage(&mut runtime, 0).await?;
        let (asked, ..) = chunk(&mut runtime).await?;
        pending.abort();
        assert_eq!(link::cancel(&mut runtime).await?, asked);
        Ok(())
    })
    .await
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_083_an_attachment_check_starts_the_owners_sandbox(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &firecracker(""), async |f| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let id = conversation.id.clone();
        let messages = format!("/api/v1/conversations/{id}/messages");
        let attach = json!({"text": "Read it", "attachments": ["uploads/a.txt"]});
        let (sent, answered) = tokio::join!(f.action(&messages, &cookie, Some("attached"), &attach), async {
            for _ in 0..100 {
                if f.backend.token(&id).is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let mut runtime = f.runtime_for(&id).await?;
            let (asked, op) = link::request(&mut runtime).await?;
            assert_eq!(op, WorkspaceOp::Stat { paths: vec!["uploads/a.txt".into()] });
            let stats = json!({"kind": "stats", "items": [{"exists": true, "kind": "file", "size": 3}]});
            link::reply(&mut runtime, asked, &[stats]).await?;
            Ok::<_, Box<dyn std::error::Error>>(runtime)
        });
        let _runtime = answered?;
        assert_eq!(sent?.status(), 202);
        Ok(())
    })
    .await
}
