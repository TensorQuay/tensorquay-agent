//! ADR-0039 slice 5: the screenshot store at the gateway.
//! - GW-I-221: a runtime's upload in chunks is kept and answered `screen.stored`; the owner reads it by its id with its
//!   type and a long private cache, another person's read is `403`, an id never stored `404`; one past 512 KiB and one
//!   whose SHA-256 isn't its id are refused; a full state quota refuses and the run goes on; the retention purge removes
//!   an old one, whose read is then `410 screenshot_expired`.
//! - GW-I-221: the retention purge reads on past files it can't remove (skipped, tried again next pass), so 500 of
//!   them never hold back the rest.
//! - GW-I-221: one quota: the run's state directory and the kept screenshots together within `sandbox.state_bytes`.
#[path = "support/fixture.rs"]
mod support;

use base64::Engine as _;
use serde_json::json;
use sha2::Digest as _;
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::{ClientFrame, ConversationId, ServerFrame, StoreRefusal};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn desktop(f: &Fixture, conversation: &ConversationId) -> TestResult {
    sqlx::query("INSERT INTO templates (name,display_name,image,capabilities,is_default,version,last_set_by) VALUES ('desktop','Desktop',$1,'{shell,browser,desktop}',false,1,'admin') ON CONFLICT (name) DO NOTHING")
        .bind(format!("sha256:{}", "4".repeat(64)))
        .execute(&f.pool)
        .await?;
    sqlx::query("UPDATE conversations SET template='desktop' WHERE id=$1")
        .bind(Uuid::parse_str(conversation.as_str())?)
        .execute(&f.pool)
        .await?;
    Ok(())
}

/// A PNG-headed image of `len` bytes, varied by `seed`, and its id.
fn image(len: usize, seed: u8) -> (Vec<u8>, String) {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.resize(len, seed);
    let id = hex(&sha2::Sha256::digest(&png));
    (png, id)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Uploads `png` as `id` in chunks of at most 96 KiB, and the gateway's answer.
async fn upload(
    runtime: &mut support::runtime::Runtime,
    id: &str,
    png: &[u8],
) -> Result<ClientFrame> {
    let chunks: Vec<&[u8]> = png.chunks(96 * 1024).collect();
    for (index, chunk) in chunks.iter().enumerate() {
        runtime
            .send_live(&ServerFrame::ScreenUpload {
                id: id.to_owned(),
                chunk: u32::try_from(index)?,
                last: index + 1 == chunks.len(),
                data: base64::engine::general_purpose::STANDARD.encode(chunk),
            })
            .await?;
    }
    runtime.action().await
}

async fn read(f: &Fixture, task: &str, id: &str, cookie: &str) -> Result<reqwest::Response> {
    Ok(f.client
        .get(f.url(&format!("/api/v1/conversations/{task}/screenshots/{id}")))
        .header("cookie", cookie)
        .send()
        .await?)
}

fn stored(id: &str, refused: Option<StoreRefusal>) -> ClientFrame {
    ClientFrame::ScreenStored {
        id: id.to_owned(),
        ok: refused.is_none(),
        reason: refused,
    }
}

// TEST: GW-I-221
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_221_an_upload_is_kept_and_read_by_its_owner(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let (_, other) = f.member("other@example.invalid").await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let mut runtime = f.runtime(&task).await?;
        let (png, id) = image(200 * 1024, 1);
        assert_eq!(upload(&mut runtime, &id, &png).await?, stored(&id, None));
        // A repeat is a no-op, answered the same.
        assert_eq!(upload(&mut runtime, &id, &png).await?, stored(&id, None));
        let response = read(f, task.id.as_str(), &id, &cookie).await?;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "image/png");
        // Never cached: a screenshot must not outlive its task's deletion in a browser's cache.
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.bytes().await?.to_vec(), png);
        assert_eq!(read(f, task.id.as_str(), &id, &other).await?.status(), 403);
        assert_eq!(
            read(f, task.id.as_str(), &"0".repeat(64), &cookie)
                .await?
                .status(),
            404
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-221
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_221_too_large_a_wrong_id_and_a_full_quota_are_refused(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[sandbox]\nstate_bytes=16777216", async |f| {
        let (owner, _) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let mut runtime = f.runtime(&task).await?;
        // Past 512 KiB: refused as the total passes it, answered once.
        let (big, big_id) = image(600 * 1024, 2);
        assert_eq!(
            upload(&mut runtime, &big_id, &big).await?,
            stored(&big_id, Some(StoreRefusal::TooLarge))
        );
        let (png, _) = image(1024, 3);
        let wrong = "1".repeat(64);
        assert_eq!(
            upload(&mut runtime, &wrong, &png).await?,
            stored(&wrong, Some(StoreRefusal::BadImage))
        );
        // The store holds 16 MiB already: the next can't fit, and the run goes on.
        for seed in 0..32_u8 {
            sqlx::query(
                "INSERT INTO screenshots (conversation_id, id, bytes) VALUES ($1, $2, 524288)",
            )
            .bind(Uuid::parse_str(task.id.as_str())?)
            .bind(hex(&sha2::Sha256::digest([seed])))
            .execute(&f.pool)
            .await?;
        }
        let (small, small_id) = image(1024, 4);
        assert_eq!(
            upload(&mut runtime, &small_id, &small).await?,
            stored(&small_id, Some(StoreRefusal::QuotaFull))
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-221
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_221_the_retention_purge_removes_an_old_screenshot(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let mut runtime = f.runtime(&task).await?;
        let (png, id) = image(4096, 5);
        assert_eq!(upload(&mut runtime, &id, &png).await?, stored(&id, None));
        sqlx::query("UPDATE screenshots SET stored_at = CURRENT_TIMESTAMP - interval '31 days' WHERE id = $1")
            .bind(&id)
            .execute(&f.pool)
            .await?;
        let cycle = f.gateway.purge_now().await?;
        assert_eq!(cycle.screenshots, 1);
        let response = read(f, task.id.as_str(), &id, &cookie).await?;
        assert_eq!(response.status(), 410);
        let body: serde_json::Value = response.json().await?;
        assert_eq!(body["error"]["code"], json!("screenshot_expired"));
        // Kept again after its purge: readable again.
        assert_eq!(upload(&mut runtime, &id, &png).await?, stored(&id, None));
        assert_eq!(read(f, task.id.as_str(), &id, &cookie).await?.status(), 200);
        Ok(())
    })
    .await
}

// TEST: GW-I-221
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_221_the_runs_state_and_the_screenshots_share_one_quota(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[sandbox]\nstate_bytes=16777216", async |f| {
        let (owner, _) = f.admin().await?;
        let task = f.conversation(owner.id).await?;
        desktop(f, &task.id).await?;
        let mut runtime = f.runtime(&task).await?;
        // The run's own state holds all but 2 KiB of the 16 MiB: a 4 KiB screenshot doesn't fit beside it.
        let state = f
            .config_storage_root()
            .join("runtime-state")
            .join(task.id.as_str());
        std::fs::create_dir_all(&state)?;
        std::fs::write(state.join("journal"), vec![0_u8; 16 * 1024 * 1024 - 2048])?;
        let (png, id) = image(4096, 6);
        assert_eq!(
            upload(&mut runtime, &id, &png).await?,
            stored(&id, Some(StoreRefusal::QuotaFull))
        );
        // The state shrinks: the same screenshot is kept.
        std::fs::write(state.join("journal"), vec![0_u8; 1024])?;
        assert_eq!(upload(&mut runtime, &id, &png).await?, stored(&id, None));
        Ok(())
    })
    .await
}

/// Stores an expired screenshot of `task` (a row and its file), stored `days` ago.
async fn expired(f: &Fixture, task: Uuid, seed: u32, days: i32) -> Result<String> {
    let id = hex(&sha2::Sha256::digest(seed.to_be_bytes()));
    sqlx::query(
        "INSERT INTO screenshots (conversation_id, id, bytes, stored_at) \
         VALUES ($1, $2, 1, CURRENT_TIMESTAMP - make_interval(days => $3))",
    )
    .bind(task)
    .bind(&id)
    .bind(days)
    .execute(&f.pool)
    .await?;
    let directory = f
        .config_storage_root()
        .join("screens")
        .join(task.to_string());
    std::fs::create_dir_all(&directory)?;
    std::fs::write(directory.join(format!("{id}.png")), b"x")?;
    Ok(id)
}

// TEST: GW-I-221
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_221_files_the_purge_cant_remove_never_hold_back_the_rest(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (owner, cookie) = f.admin().await?;
        let stuck = f.conversation(owner.id).await?;
        let free = f.conversation(owner.id).await?;
        let (stuck_id, free_id) = (
            Uuid::parse_str(stuck.id.as_str())?,
            Uuid::parse_str(free.id.as_str())?,
        );
        // 500 of the oldest expired files that can't be removed: a whole page that can't go. Each is a directory in its
        // file's place, which unlinking refuses (EISDIR) whoever the gateway runs as; a read-only directory wouldn't
        // do, as the coverage lane runs the tests as its user namespace's root, which writes through any mode.
        let directory = f
            .config_storage_root()
            .join("screens")
            .join(stuck_id.to_string());
        let mut stuck_files = Vec::new();
        for seed in 0..500 {
            let file = directory.join(format!("{}.png", expired(f, stuck_id, seed, 60).await?));
            std::fs::remove_file(&file)?;
            std::fs::create_dir(&file)?;
            stuck_files.push(file);
        }
        let later = expired(f, free_id, 1_000, 40).await?;
        let cycle = f.gateway.purge_now().await;
        for file in &stuck_files {
            std::fs::remove_dir(file)?;
            std::fs::write(file, b"x")?;
        }
        assert_eq!(cycle?.screenshots, 1, "the one behind them is purged");
        assert_eq!(
            read(f, free.id.as_str(), &later, &cookie).await?.status(),
            410
        );
        let left: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM screenshots WHERE conversation_id = $1 AND purged_at IS NULL",
        )
        .bind(stuck_id)
        .fetch_one(&f.pool)
        .await?;
        assert_eq!(left, 500, "kept for the next pass");
        // Removable again: the next pass takes them.
        assert_eq!(f.gateway.purge_now().await?.screenshots, 500);
        Ok(())
    })
    .await
}
