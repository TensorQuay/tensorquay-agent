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

/// The member's read of message `seq`: its first file's path in the member's workspace.
async fn read_path(f: &Fixture, agent: &Agent, channel: &str, seq: u64) -> Result<String> {
    let (status, page) = channel_agent::runtime_call(
        f,
        Method::GET,
        &format!("/runtime/channels/{channel}/messages?since={}", seq - 1),
        &agent.token(f)?,
        None,
    )
    .await?;
    assert_eq!(status, 200, "{page}");
    Ok(page["messages"][0]["files"][0]["path"]
        .as_str()
        .ok_or_else(|| format!("no path: {page}"))?
        .to_owned())
}

/// The path the wake's sentence names that starts with `start` (an `uploads/…` path).
fn named_path(sentence: &str, start: &str) -> Option<String> {
    sentence
        .split([' ', ',', ';', '(', ')'])
        .map(|word| word.trim_end_matches('.'))
        .find(|word| word.starts_with(start))
        .map(str::to_owned)
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
        // The wake says a file waits and from which message, never its path (SEC-068); the read gives the path.
        let sentence = analyst.woken(f, &id, 0).await?;
        assert!(
            sentence.ends_with(&format!(
                " 1 file waits in your workspace, from message {seq}; channel_read lists its path."
            )),
            "{sentence}"
        );
        assert!(
            named_path(&sentence, "uploads/").is_none() && !sentence.contains("q3"),
            "{sentence}"
        );
        let path = read_path(f, &analyst, &id, seq).await?;
        assert_eq!(path, format!("uploads/pricing-{seq}-q3.pdf"));
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
        analyst.woken(f, &id, seq).await?;
        let placed = read_path(f, &analyst, &id, second).await?;
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
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(placed.len(), 3, "{placed:?}");
        for name in &placed {
            assert!(!name.starts_with('.') && !name.contains(".."), "{name}");
        }
        // SEC-068: neither the channel's name nor a file's reaches the wake's turn; it counts them.
        for planted in ["pricing", "passwd", "bashrc", "a\u{7}b", "uploads/"] {
            assert!(!sentence.contains(planted), "{planted} in {sentence}");
        }
        assert!(
            sentence.contains("3 files wait in your workspace, from message 1;"),
            "{sentence}"
        );
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

// TEST: GW-I-363
// A channel upload has an upload's body limit, not the API's default (review of 20 Nov 2026): with the default
// configuration a 2 MiB file uploads and downloads byte for byte, and one over files.upload_bytes is 413.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_363_a_channel_upload_has_the_upload_body_limit(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = channel(f, &cookie, "pricing", &[]).await?;
        let bytes: Vec<u8> = (0..2 * 1024 * 1024)
            .map(|n| u8::try_from(n % 251).unwrap_or(0))
            .collect();
        let file = uploaded(f, &cookie, &id, "big.bin", &bytes).await?;
        let (status, posted) = post_with(f, &cookie, &id, text("the big one"), &[file]).await?;
        assert_eq!(status, 201, "{posted}");
        let seq = posted["seq"].as_u64().ok_or("seq")?;
        let response = f
            .get(
                &format!("/api/v1/channels/{id}/messages/{seq}/files/0"),
                &cookie,
            )
            .await?;
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.bytes().await?.to_vec(), bytes, "byte for byte");
        let (status, answer) =
            upload(f, &cookie, &id, "too.bin", vec![0; 25 * 1024 * 1024 + 1]).await?;
        assert_eq!(
            (status, &answer["error"]["code"]),
            (413, &json!("upload_too_large")),
            "{answer}"
        );
        Ok(())
    }))
    .await
}

// TEST: GW-I-364
// The composer's remove (review of 20 Nov 2026): a person deletes their own upload that no post took, and its room in
// the channel's total comes back; anyone else's, or one already posted, isn't theirs to delete.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_364_a_person_deletes_their_own_unposted_upload(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(pool, None, SMALL, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = channel(f, &cookie, "pricing", &[]).await?;
        let mut ids = Vec::new();
        for n in 0..4 {
            ids.push(uploaded(f, &cookie, &id, &format!("part-{n}.bin"), &[1; 1000]).await?);
        }
        assert_eq!(
            upload(f, &cookie, &id, "more.bin", vec![1; 1000]).await?.0,
            409,
            "full"
        );
        let delete = |upload: String, cookie: String| {
            let id = id.clone();
            async move {
                call(
                    f,
                    Method::DELETE,
                    &format!("/api/v1/channels/{id}/uploads/{upload}"),
                    &cookie,
                    (None, None),
                )
                .await
            }
        };
        assert_eq!(delete(ids[3].clone(), cookie.clone()).await?.0, 204);
        assert_eq!(delete(ids[3].clone(), cookie.clone()).await?.0, 404, "gone");
        assert_eq!(
            upload(f, &cookie, &id, "more.bin", vec![1; 1000]).await?.0,
            201,
            "its room is back"
        );
        // Someone else's: not found to them. Posted: not deletable.
        let (member, other) = f.member("colleague@example.invalid").await?;
        let (status, _) = call(
            f,
            Method::PUT,
            &format!("/api/v1/channels/{id}/members/user/{}", member.id),
            &cookie,
            (Some(json!({"role": "member"})), None),
        )
        .await?;
        assert_eq!(status, 200);
        assert_eq!(delete(ids[0].clone(), other).await?.0, 404);
        assert_eq!(
            post_with(f, &cookie, &id, text("posted"), &ids[1..2])
                .await?
                .0,
            201
        );
        assert_eq!(delete(ids[1].clone(), cookie.clone()).await?.0, 404);
        let stored = f
            .config_storage_root()
            .join("channel-files")
            .join(&id)
            .join("uploads")
            .join(&ids[3]);
        assert!(!stored.exists(), "its bytes went with it");
        Ok(())
    }))
    .await
}

// TEST: GW-I-365
// Uploads no post took are swept after 24 hours, bytes and row (review of 20 Nov 2026); a newer one stays.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_365_unposted_uploads_older_than_a_day_are_swept(pool: PgPool) -> TestResult {
    Box::pin(support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = channel(f, &cookie, "pricing", &[]).await?;
        let old = uploaded(f, &cookie, &id, "old.bin", b"old").await?;
        let new = uploaded(f, &cookie, &id, "new.bin", b"new").await?;
        sqlx::query("UPDATE channel_uploads SET created_at = CURRENT_TIMESTAMP - interval '25 hours' WHERE id = $1")
            .bind(Uuid::parse_str(&old)?)
            .execute(&f.pool)
            .await?;
        f.gateway.channel_uploads_sweep_now().await?;
        let left: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM channel_uploads WHERE channel_id = $1")
            .bind(Uuid::parse_str(&id)?)
            .fetch_all(&f.pool)
            .await?;
        assert_eq!(left, vec![Uuid::parse_str(&new)?]);
        let uploads = f.config_storage_root().join("channel-files").join(&id).join("uploads");
        assert!(!uploads.join(&old).exists() && uploads.join(&new).exists());
        Ok(())
    }))
    .await
}
