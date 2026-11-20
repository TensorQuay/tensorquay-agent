//! Files on a channel's posts (ADR-0014's amendment of 20 Nov 2026, A9): the composer's limits (GW-I-356); a post with
//! files and its download (GW-I-357); delivery into a woken agent member's workspace, flat in `uploads/` as
//! `<channel>-<seq>-<file>`, never overwriting, the wake naming each exact path, a rename moving nothing (GW-I-358);
//! the member's read naming the placed paths, a person's naming none (GW-I-359); a name that can't leave `uploads/`
//! (SEC-069).
#[path = "support/channel_agent.rs"]
mod channel_agent;
#[path = "support/fixture.rs"]
mod support;
use channel_agent::{Agent, Result, channel, text};
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use uuid::Uuid;

/// Uploads `bytes` as `name` to the channel; the status and the answer.
async fn upload(
    f: &Fixture,
    cookie: &str,
    channel: &str,
    name: &str,
    bytes: Vec<u8>,
) -> Result<(u16, Value)> {
    let mut url = reqwest::Url::parse(&f.url(&format!("/api/v1/channels/{channel}/uploads")))?;
    url.query_pairs_mut().append_pair("name", name);
    let response = f
        .client
        .post(url)
        .header("origin", f.origin())
        .header("cookie", cookie)
        .body(bytes)
        .send()
        .await?;
    let status = response.status().as_u16();
    let text = response.text().await?;
    Ok((
        status,
        if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text)?
        },
    ))
}

async fn uploaded(
    f: &Fixture,
    cookie: &str,
    channel: &str,
    name: &str,
    bytes: &[u8],
) -> Result<String> {
    let (status, answer) = upload(f, cookie, channel, name, bytes.to_vec()).await?;
    assert_eq!(status, 201, "{answer}");
    Ok(answer["id"].as_str().ok_or("id")?.to_owned())
}

/// A person's post with files; the status and the answer.
async fn post_with(
    f: &Fixture,
    cookie: &str,
    channel: &str,
    body: Value,
    files: &[String],
) -> Result<(u16, Value)> {
    let key = Uuid::now_v7().to_string();
    call(
        f,
        Method::POST,
        &format!("/api/v1/channels/{channel}/messages"),
        cookie,
        (
            Some(json!({"body": body, "files": files})),
            Some(key.as_str()),
        ),
    )
    .await
}

const SMALL: &str = "\n[files]\nupload_bytes = 1024\nuploads_bytes = 4096\n";

// TEST: GW-I-356
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_356_a_channels_files_keep_the_composers_limits(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(pool, None, SMALL, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = channel(f, &cookie, "pricing", &[]).await?;
        // Over the per-file limit: refused, nothing kept.
        let (status, answer) = upload(f, &cookie, &id, "big.bin", vec![0; 2000]).await?;
        assert_eq!(
            (status, &answer["error"]["code"]),
            (413, &json!("upload_too_large")),
            "{answer}"
        );
        // Four of 1000 bytes fit the channel's 4096; a fifth is refused.
        let mut ids = Vec::new();
        for n in 0..4 {
            ids.push(uploaded(f, &cookie, &id, &format!("part-{n}.bin"), &[1; 1000]).await?);
        }
        let (status, answer) = upload(f, &cookie, &id, "part-4.bin", vec![1; 1000]).await?;
        assert_eq!(
            (status, &answer["error"]["code"]),
            (409, &json!("uploads_full")),
            "{answer}"
        );
        // A person who can't see the channel can't upload to it.
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        assert_eq!(
            upload(f, &stranger, &id, "x.txt", b"x".to_vec()).await?.0,
            404
        );
        // More than 32 files, an unknown id, or another channel's upload: refused, nothing posted.
        let many: Vec<String> = (0..33).map(|_| ids[0].clone()).collect();
        assert_eq!(
            post_with(f, &cookie, &id, text("too many"), &many).await?.0,
            400
        );
        assert_eq!(
            post_with(
                f,
                &cookie,
                &id,
                text("unknown"),
                &[Uuid::now_v7().to_string()]
            )
            .await?
            .0,
            400
        );
        let other = channel(f, &cookie, "launch", &[]).await?;
        let theirs = uploaded(f, &cookie, &other, "elsewhere.txt", b"x").await?;
        assert_eq!(
            post_with(f, &cookie, &id, text("not here"), &[theirs])
                .await?
                .0,
            400
        );
        // Posted once, an upload can't be posted again.
        assert_eq!(
            post_with(f, &cookie, &id, text("once"), &ids[..1]).await?.0,
            201
        );
        assert_eq!(
            post_with(f, &cookie, &id, text("twice"), &ids[..1])
                .await?
                .0,
            400
        );
        Ok(())
    }))
    .await
}

// TEST: GW-I-357
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_357_a_post_carries_its_files_and_people_download_them(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = channel(f, &cookie, "pricing", &[]).await?;
        let pdf = b"%PDF-1.7 the price list".to_vec();
        let file = uploaded(f, &cookie, &id, "q3.pdf", &pdf).await?;
        let (status, posted) = post_with(
            f,
            &cookie,
            &id,
            json!({"type": "proposal", "title": "Q3 prices", "text": "See the file."}),
            &[file],
        )
        .await?;
        assert_eq!(status, 201, "{posted}");
        assert_eq!(
            posted["files"],
            json!([{"name": "q3.pdf", "size": pdf.len()}])
        );
        let seq = posted["seq"].as_u64().ok_or("seq")?;
        // People read it with its files, and download each.
        let (_, page) = call(
            f,
            Method::GET,
            &format!("/api/v1/channels/{id}/messages?since=0"),
            &cookie,
            (None, None),
        )
        .await?;
        assert_eq!(
            page["messages"][0]["files"],
            json!([{"name": "q3.pdf", "size": pdf.len()}])
        );
        let response = f
            .get(
                &format!("/api/v1/channels/{id}/messages/{seq}/files/0"),
                &cookie,
            )
            .await?;
        assert_eq!(response.status().as_u16(), 200);
        let disposition = response
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        assert!(disposition.starts_with("attachment"), "{disposition}");
        assert_eq!(response.bytes().await?.to_vec(), pdf);
        assert_eq!(
            f.get(
                &format!("/api/v1/channels/{id}/messages/{seq}/files/1"),
                &cookie
            )
            .await?
            .status()
            .as_u16(),
            404
        );
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        assert_eq!(
            f.get(
                &format!("/api/v1/channels/{id}/messages/{seq}/files/0"),
                &stranger
            )
            .await?
            .status()
            .as_u16(),
            404
        );
        Ok(())
    }))
    .await
}

/// The path the wake's sentence names for `name` (an `uploads/…` path ending in it).
fn named_path(sentence: &str, name: &str) -> Option<String> {
    sentence
        .split([' ', ',', ';', '(', ')'])
        .find(|word| word.starts_with("uploads/") && word.trim_end_matches('.').ends_with(name))
        .map(|word| word.trim_end_matches('.').to_owned())
}

// TEST: GW-I-358
// TEST: GW-I-359
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_358_359_a_woken_member_gets_the_files_where_its_wake_says(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let mut analyst = Agent::new(f, owner.id, "Pricing analyst").await?;
        let id = channel(f, &cookie, "pricing", &[(&analyst, "member")]).await?;
        // A file of the same name already sits where the first would go: it is never overwritten.
        let pdf = b"%PDF-1.7 the price list".to_vec();
        let file = uploaded(f, &cookie, &id, "q3.pdf", &pdf).await?;
        let (status, posted) = post_with(
            f,
            &cookie,
            &id,
            json!({"type": "proposal", "title": "Q3 prices", "text": "See the file."}),
            &[file],
        )
        .await?;
        assert_eq!(status, 201, "{posted}");
        let seq = posted["seq"].as_u64().ok_or("seq")?;
        let sentence = analyst.woken(f, &id, 0).await?;
        let path =
            named_path(&sentence, "q3.pdf").ok_or_else(|| format!("no path in: {sentence}"))?;
        assert_eq!(path, format!("uploads/pricing-{seq}-q3.pdf"), "{sentence}");
        let workspace = f.backend.mounted(&analyst.id).ok_or("workspace")?;
        assert_eq!(std::fs::read(workspace.join(&path))?, pdf);
        // Its read names the same path; a person's read names none.
        analyst.start(f).await?;
        let (status, page) = channel_agent::runtime_call(
            f,
            Method::GET,
            &format!("/runtime/channels/{id}/messages?since=0"),
            &analyst.token(f)?,
            None,
        )
        .await?;
        assert_eq!(status, 200, "{page}");
        assert_eq!(
            page["messages"][0]["files"],
            json!([{"name": "q3.pdf", "size": pdf.len(), "path": path}])
        );
        let (_, people) = call(
            f,
            Method::GET,
            &format!("/api/v1/channels/{id}/messages?since=0"),
            &cookie,
            (None, None),
        )
        .await?;
        assert!(
            people["messages"][0]["files"][0].get("path").is_none(),
            "{people}"
        );
        analyst.end(f).await?;
        // Renamed, the channel moves nothing already placed; a new post's file of the same name goes beside it.
        sqlx::query("UPDATE channels SET name = 'pricing-renamed' WHERE id = $1")
            .bind(Uuid::parse_str(&id)?)
            .execute(&f.pool)
            .await?;
        std::fs::write(
            workspace.join(format!("uploads/pricing-renamed-{}-q3.pdf", seq + 1)),
            b"mine",
        )?;
        let file = uploaded(f, &cookie, &id, "q3.pdf", b"second").await?;
        let (_, again) = post_with(
            f,
            &cookie,
            &id,
            text("@pricing analyst the second one"),
            &[file],
        )
        .await?;
        let second = again["seq"].as_u64().ok_or("seq")?;
        assert_eq!(second, seq + 1);
        let sentence = analyst.woken(f, &id, seq).await?;
        let placed =
            named_path(&sentence, "q3.pdf").ok_or_else(|| format!("no path in: {sentence}"))?;
        assert_ne!(
            placed,
            format!("uploads/pricing-renamed-{second}-q3.pdf"),
            "never over the file already there"
        );
        assert!(
            placed.starts_with(&format!("uploads/pricing-renamed-{second}-q3")),
            "{placed}"
        );
        assert_eq!(std::fs::read(workspace.join(&placed))?, b"second");
        assert_eq!(
            std::fs::read(workspace.join(format!("uploads/pricing-renamed-{second}-q3.pdf")))?,
            b"mine"
        );
        assert_eq!(
            std::fs::read(workspace.join(&path))?,
            pdf,
            "the first stays where it was"
        );
        Ok(())
    }))
    .await
}

// TEST: SEC-069
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn sec_069_a_channel_files_name_cant_leave_uploads(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let mut analyst = Agent::new(f, owner.id, "Pricing analyst").await?;
        let id = channel(f, &cookie, "../pricing", &[(&analyst, "member")]).await?;
        let mut files = Vec::new();
        for name in ["../../etc/passwd", ".bashrc", "a\u{7}b.txt"] {
            let (status, answer) = upload(f, &cookie, &id, name, b"x".to_vec()).await?;
            assert_eq!(status, 201, "{answer}");
            let stored = answer["name"].as_str().ok_or("name")?;
            assert!(
                !stored.contains('/') && !stored.starts_with('.') && !stored.contains('\u{7}'),
                "{stored}"
            );
            files.push(answer["id"].as_str().ok_or("id")?.to_owned());
        }
        let (status, _) = post_with(
            f,
            &cookie,
            &id,
            text("@pricing analyst three files"),
            &files,
        )
        .await?;
        assert_eq!(status, 201);
        let sentence = analyst.woken(f, &id, 0).await?;
        let workspace = f.backend.mounted(&analyst.id).ok_or("workspace")?;
        let uploads = workspace.join("uploads");
        let placed: Vec<String> = std::fs::read_dir(&uploads)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(placed.len(), 3, "{placed:?}");
        for name in &placed {
            assert!(!name.starts_with('.') && !name.contains(".."), "{name}");
            assert!(
                sentence.contains(&format!("uploads/{name}")),
                "{name} in {sentence}"
            );
        }
        assert!(
            !workspace.join("etc").exists()
                && !workspace
                    .parent()
                    .ok_or("parent")?
                    .join("etc/passwd")
                    .exists()
        );
        Ok(())
    }))
    .await
}
