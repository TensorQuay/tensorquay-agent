//! GW-I-356 (ADR-0041): a person's own changes to a workspace's memory notes over HTTP. A note is written, removed
//! and undone under `.memory/`, listed and read by the files operations; a write share is required (the administrator
//! role grants no content); a name that can't be a note and a text that holds what looks like a key are refused, and
//! nothing is written.
#[path = "support/files_api.rs"]
mod files_api;
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use support::{Fixture, TestResult};

use files_api::{body, call, code, task};

fn note(
    f: &Fixture,
    cookie: &str,
    method: Method,
    task: &str,
    name: &str,
) -> reqwest::RequestBuilder {
    call(
        f,
        cookie,
        method,
        &format!("/api/v1/conversations/{task}/memory/{name}"),
    )
}

fn undo(f: &Fixture, cookie: &str, task: &str, name: &str) -> reqwest::RequestBuilder {
    call(
        f,
        cookie,
        Method::POST,
        &format!("/api/v1/conversations/{task}/memory/{name}/undo"),
    )
}

async fn read(
    f: &Fixture,
    cookie: &str,
    task: &str,
    name: &str,
) -> files_api::Result<(u16, String)> {
    let response = call(
        f,
        cookie,
        Method::GET,
        &format!("/api/v1/conversations/{task}/files/download"),
    )
    .query(&[("path", format!(".memory/{name}"))])
    .send()
    .await?;
    Ok((response.status().as_u16(), response.text().await?))
}

/// A made-up key, generated here so the source holds nothing key-shaped for the secrets scan (G-10).
fn made_up_key() -> String {
    const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut state: u64 = 9;
    let text: String = (0..40)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            char::from(BASE62[usize::try_from(state % 62).unwrap_or(0)])
        })
        .collect();
    format!("sk-{text}")
}

// TEST: GW-I-356
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_356_a_note_is_written_listed_edited_undone_and_removed(pool: PgPool) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let id = task(f, &cookie).await?;
        let first = "about: The build command is make all\nkind: convention\n\nRun make all.\n";
        let written = note(f, &cookie, Method::PUT, &id, "build.md")
            .body(first)
            .send()
            .await?;
        assert_eq!(written.status(), 204);
        // Listed and read by the files operations: `.memory/` is a folder of the workspace.
        let (status, listing) = body(
            call(
                f,
                &cookie,
                Method::GET,
                &format!("/api/v1/conversations/{id}/files"),
            )
            .query(&[("path", ".memory")])
            .send()
            .await?,
        )
        .await?;
        assert_eq!(status, 200, "{listing}");
        let names: Vec<_> = listing["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["name"].as_str())
            .collect();
        assert!(names.contains(&"build.md"), "{listing}");
        assert_eq!(
            read(f, &cookie, &id, "build.md").await?,
            (200, first.to_owned())
        );
        // An edit, then its Undo: the previous version is back; a second Undo has nothing kept and removes nothing new.
        let second = "about: The build command is make release\n";
        assert_eq!(
            note(f, &cookie, Method::PUT, &id, "build.md")
                .body(second)
                .send()
                .await?
                .status(),
            204
        );
        assert_eq!(read(f, &cookie, &id, "build.md").await?.1, second);
        let (status, undone) = body(undo(f, &cookie, &id, "build.md").send().await?).await?;
        assert_eq!((status, &undone), (200, &json!({"outcome": "restored"})));
        assert_eq!(read(f, &cookie, &id, "build.md").await?.1, first);
        // The same Undo sent again (a retried request) finds nothing kept: 404, and the restored note stays.
        assert_eq!(
            code(undo(f, &cookie, &id, "build.md").send().await?)
                .await?
                .0,
            404
        );
        assert_eq!(read(f, &cookie, &id, "build.md").await?.1, first);
        // Removed, and brought back by an Undo.
        assert_eq!(
            note(f, &cookie, Method::DELETE, &id, "build.md")
                .send()
                .await?
                .status(),
            204
        );
        assert_eq!(read(f, &cookie, &id, "build.md").await?.0, 404);
        assert_eq!(
            body(undo(f, &cookie, &id, "build.md").send().await?)
                .await?
                .1,
            json!({"outcome": "restored"})
        );
        assert_eq!(read(f, &cookie, &id, "build.md").await?.1, first);
        // A new note's Undo removes it; then there is nothing to undo or remove.
        assert_eq!(
            note(f, &cookie, Method::PUT, &id, "new.md")
                .body("about: new\n")
                .send()
                .await?
                .status(),
            204
        );
        assert_eq!(
            body(undo(f, &cookie, &id, "new.md").send().await?).await?.1,
            json!({"outcome": "removed"})
        );
        assert_eq!(
            code(undo(f, &cookie, &id, "new.md").send().await?).await?.0,
            404
        );
        assert_eq!(
            code(
                note(f, &cookie, Method::DELETE, &id, "new.md")
                    .send()
                    .await?
            )
            .await?
            .0,
            404
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-356
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_356_keys_bad_names_and_readers_are_refused_and_nothing_is_written(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, admin) = f.admin().await?;
        let (_, owner) = f.member("owner@example.invalid").await?;
        let (reader, reader_cookie) = f.member("reader@example.invalid").await?;
        let id = task(f, &owner).await?;
        // A text that holds what looks like a key: refused, nothing written.
        let leak = format!("about: the deploy key\n\nexport TOKEN={}\n", made_up_key());
        let refused = note(f, &owner, Method::PUT, &id, "deploy.md")
            .body(leak)
            .send()
            .await?;
        assert_eq!(code(refused).await?, (400, "invalid_request".into()));
        assert_eq!(read(f, &owner, &id, "deploy.md").await?.0, 404);
        // A name that can't be a note.
        for name in [".hidden.md", "notes.txt", "a%2Fb.md", "..%2Fescape.md"] {
            let response = note(f, &owner, Method::PUT, &id, name)
                .body("x")
                .send()
                .await?;
            assert_eq!(response.status(), 400, "{name}");
        }
        // The administrator role grants no content; a read share may not write, remove or undo.
        assert_eq!(
            note(f, &owner, Method::PUT, &id, "ok.md")
                .body("about: ok\n")
                .send()
                .await?
                .status(),
            204
        );
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
        let shared = call(
            f,
            &owner,
            Method::PUT,
            &format!("/api/v1/workspaces/{workspace}/shares/{}", reader.id),
        )
        .json(&json!({"access": "read"}))
        .send()
        .await?;
        assert_eq!(shared.status(), 200);
        for cookie in [&admin, &reader_cookie] {
            assert_eq!(
                note(f, cookie, Method::PUT, &id, "ok.md")
                    .body("about: changed\n")
                    .send()
                    .await?
                    .status(),
                403
            );
            assert_eq!(
                note(f, cookie, Method::DELETE, &id, "ok.md")
                    .send()
                    .await?
                    .status(),
                403
            );
            assert_eq!(undo(f, cookie, &id, "ok.md").send().await?.status(), 403);
        }
        assert_eq!(read(f, &owner, &id, "ok.md").await?.1, "about: ok\n");
        // A cookie request without the Origin is refused, as every changing request is.
        let no_origin = f
            .client
            .put(f.url(&format!("/api/v1/conversations/{id}/memory/ok.md")))
            .header("cookie", &owner)
            .body("about: x\n")
            .send()
            .await?;
        assert_eq!(no_origin.status(), 403);
        Ok(())
    })
    .await
}
