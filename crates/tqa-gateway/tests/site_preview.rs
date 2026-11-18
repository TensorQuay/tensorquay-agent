//! GW-I-321: a workspace folder served as a site over HTTP, by a pass. The owner mints a pass for the folder that
//! holds the page; the site's files are read under it without a cookie, each with its type, the page with the site's
//! policy naming its own root; a route of the site answers the page; a path that climbs, an unknown pass and another
//! person's mint are refused; a folder without a page mints too and its page opens by name; the cap of passes holds.
#[path = "support/files_api.rs"]
mod files_api;
#[path = "support/fixture.rs"]
mod support;

use files_api::{Result, body, call, code, task_with};
use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use support::{Fixture, TestResult};

const PAGE: &[u8] = b"<!doctype html><html><head><link rel=\"stylesheet\" href=\"style.css\"><script type=\"module\" src=\"app.js\"></script></head><body><h1>Hello</h1></body></html>";

async fn mint(
    f: &Fixture,
    cookie: &str,
    task: &str,
    folder: &str,
) -> Result<(u16, serde_json::Value)> {
    body(
        call(
            f,
            cookie,
            Method::POST,
            &format!("/api/v1/conversations/{task}/site"),
        )
        .json(&json!({"folder": folder}))
        .send()
        .await?,
    )
    .await
}

/// A read under the pass: no cookie, as the frame's own requests go.
async fn fetch(f: &Fixture, url: &str) -> Result<reqwest::Response> {
    Ok(f.client.get(f.url(url)).send().await?)
}

// TEST: GW-I-321
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_321_a_folder_is_served_as_a_site_by_its_pass(pool: PgPool) -> TestResult {
    support::run(pool, async |f: &Fixture| {
        f.admin().await?;
        let (_, owner) = f.member("owner@example.invalid").await?;
        let (_, other) = f.member("other@example.invalid").await?;
        let task = task_with(
            f,
            &owner,
            &[
                ("index.html", PAGE.to_vec()),
                ("style.css", b"h1 { color: teal }".to_vec()),
                ("app.js", b"export const ready = true;".to_vec()),
            ],
        )
        .await?;
        // The uploads land in uploads/: that folder is the site.
        let (status, minted) = mint(f, &owner, &task, "uploads").await?;
        assert_eq!(status, 201, "{minted}");
        let pass = minted["pass"].as_str().ok_or("pass")?;
        assert_eq!(pass.len(), 43);
        let url = minted["url"].as_str().ok_or("url")?.to_owned();
        assert_eq!(url, format!("/api/v1/conversations/{task}/site/{pass}/"));
        assert_eq!(minted["expires_in_seconds"], 900);
        // What the network setting lets the page reach: the test stack's list has no rule, so nothing.
        assert_eq!(minted["network"], json!({"any": false, "hosts": []}));
        // The page, without a cookie, with the site's policy naming its own root under the pass.
        let page = fetch(f, &url).await?;
        assert_eq!(page.status(), 200);
        let kind = page
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        assert_eq!(kind, "text/html; charset=utf-8");
        let policy = page
            .headers()
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        assert!(
            policy.starts_with("sandbox allow-scripts; default-src 'none'; script-src "),
            "{policy}"
        );
        assert!(
            policy.contains(&format!("{}{url} 'unsafe-inline'", f.origin())),
            "{policy}"
        );
        assert!(!policy.contains("allow-same-origin"));
        assert_eq!(
            page.headers()
                .get("x-content-type-options")
                .and_then(|v| v.to_str().ok()),
            Some("nosniff")
        );
        // The page as it came, with the reporter first in its head (18.1, GW-U-222); its relative paths unchanged.
        let served = page.text().await?;
        let head = served.find("<head>").ok_or("head")? + "<head>".len();
        assert!(
            served[head..].starts_with("<script>(()=>{const B="),
            "{served}"
        );
        let end = head + served[head..].find("</script>").ok_or("reporter")? + "</script>".len();
        assert_eq!(
            format!("{}{}", &served[..head], &served[end..]).as_bytes(),
            PAGE
        );
        // Its files, by their types; a route of the site answers the page.
        for (file, kind, starts) in [
            ("style.css", "text/css; charset=utf-8", "h1 {"),
            ("app.js", "text/javascript; charset=utf-8", "export"),
            ("index.html", "text/html; charset=utf-8", "<!doctype"),
            ("about/team", "text/html; charset=utf-8", "<!doctype"),
            ("missing.css", "text/html; charset=utf-8", "<!doctype"),
        ] {
            let answer = fetch(f, &format!("{url}{file}")).await?;
            assert_eq!(answer.status(), 200, "{file}");
            assert_eq!(
                answer
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok()),
                Some(kind),
                "{file}"
            );
            assert!(
                answer.headers().get("content-security-policy").is_some()
                    == (kind.starts_with("text/html")),
                "{file}"
            );
            // The page's origin is opaque (sandbox without allow-same-origin), so its own styles, scripts and images
            // are cross-origin loads to the browser: a same-origin resource policy would refuse every one of them
            // (the previews stack, 14 Nov 2026: the page drew unstyled and ran nothing).
            assert_eq!(
                answer
                    .headers()
                    .get("cross-origin-resource-policy")
                    .and_then(|v| v.to_str().ok()),
                Some("cross-origin"),
                "{file}"
            );
            assert!(answer.text().await?.starts_with(starts), "{file}");
        }
        // A module script is fetched in CORS mode from the page's opaque origin (`Origin: null`), as a built site's
        // entry is: the file answers it with the open allow-origin (the pass guards the file; the previews stack of
        // 14 Nov 2026: the built site's entry never ran without it).
        let module = f
            .client
            .get(f.url(&format!("{url}app.js")))
            .header("origin", "null")
            .header("sec-fetch-mode", "cors")
            .send()
            .await?;
        assert_eq!(module.status(), 200);
        assert_eq!(
            module
                .headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("*")
        );
        // Climbing, an unknown pass and a pass on another task are all missing, alike.
        assert_eq!(fetch(f, &format!("{url}../../.tqa/x")).await?.status(), 404);
        assert_eq!(
            fetch(
                f,
                &format!(
                    "/api/v1/conversations/{task}/site/{}/index.html",
                    "x".repeat(43)
                )
            )
            .await?
            .status(),
            404
        );
        // Another person cannot mint a pass on the task; a folder that doesn't exist is refused; the root always exists.
        let (status, _) = mint(f, &other, &task, "uploads").await?;
        assert!(status == 403 || status == 404, "{status}");
        let (status, refused) = code(
            call(
                f,
                &owner,
                Method::POST,
                &format!("/api/v1/conversations/{task}/site"),
            )
            .json(&json!({"folder": "nowhere"}))
            .send()
            .await?,
        )
        .await?;
        assert_eq!((status, refused.as_str()), (404, "not_found"));
        let (status, root) = mint(f, &owner, &task, "").await?;
        assert_eq!(status, 201, "{root}");
        // A folder without index.html mints too: an agent's report.html beside other files opens by its name, and the
        // folder's root, with no page to answer, is missing.
        let report = task_with(
            f,
            &owner,
            &[
                ("report.html", b"<!doctype html><p>Report</p>".to_vec()),
                ("data.csv", b"a,b\n".to_vec()),
            ],
        )
        .await?;
        let (status, minted) = mint(f, &owner, &report, "uploads").await?;
        assert_eq!(status, 201, "{minted}");
        let url = minted["url"].as_str().ok_or("url")?.to_owned();
        let page = fetch(f, &format!("{url}report.html")).await?;
        assert_eq!(page.status(), 200);
        assert!(page.text().await?.contains("Report"));
        assert_eq!(
            fetch(f, &url).await?.status(),
            404,
            "no index.html to answer the root"
        );
        // The cap: 8 live passes per task, the 9th refused. Two are live already (the uploads folder and the root).
        for _ in 0..6 {
            assert_eq!(mint(f, &owner, &task, "uploads").await?.0, 201);
        }
        let (status, refused) = code(
            call(
                f,
                &owner,
                Method::POST,
                &format!("/api/v1/conversations/{task}/site"),
            )
            .json(&json!({"folder": "uploads"}))
            .send()
            .await?,
        )
        .await?;
        assert_eq!((status, refused.as_str()), (409, "site_passes_limit"));
        Ok(())
    })
    .await
}

/// The workspace's own folder on the host, for writing a built app as a build writes it.
fn workspace_root(
    f: &Fixture,
    conversation: &tqa_types::ConversationId,
) -> Result<std::path::PathBuf> {
    let (mount, _) = f.backend.mounts(conversation).ok_or("mounts")?;
    let tqa_sandbox::WorkspaceSource::HostDirectory(root) = mount.source else {
        return Err("a host-directory workspace".into());
    };
    Ok(root)
}

fn write(root: &std::path::Path, files: &[(&str, &str)]) -> Result<()> {
    for (name, text) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().ok_or("parent")?)?;
        std::fs::write(path, text)?;
    }
    Ok(())
}

// TEST: GW-I-321
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_321_built_apps_preview_as_they_are(pool: PgPool) -> TestResult {
    support::run(pool, async |f: &Fixture| {
        let (user, cookie) = f.admin().await?;
        let conversation = f.conversation(user.id).await?;
        let task = conversation.id.to_string();
        let _runtime = f.runtime(&conversation).await?;
        let root = workspace_root(f, &conversation.id)?;
        // A default Vite build: root paths in the page and in its stylesheet, and in a string of its script.
        let script = "const logo = \"/assets/logo.png\"; import(\"./about-1.js\");";
        write(
            &root,
            &[
                (
                    "vite/dist/index.html",
                    "<!doctype html><html><head><script type=\"module\" crossorigin src=\"/assets/index-1.js\"></script>\
<link rel=\"stylesheet\" href=\"/assets/index-1.css\"></head><body><div id=\"root\"></div></body></html>",
                ),
                ("vite/dist/assets/index-1.css", "body{background:url(/assets/bg.png)}"),
                ("vite/dist/assets/index-1.js", script),
                // A relative build: nothing to move.
                (
                    "relative/dist/index.html",
                    "<!doctype html><html><head><script src=\"./assets/a.js\"></script></head><body></body></html>",
                ),
                ("relative/dist/assets/a.css", "body{background:url(./bg.png)}"),
                // A two-page static export that links `/about` and writes `about.html`.
                ("export/out/index.html", "<!doctype html><html><head></head><body><a href=\"/about\">About</a></body></html>"),
                ("export/out/about.html", "<!doctype html><html><head></head><body><h1>About us</h1></body></html>"),
            ],
        )?;
        let (status, minted) = mint(f, &cookie, &task, "vite/dist").await?;
        assert_eq!(status, 201, "{minted}");
        let url = minted["url"].as_str().ok_or("url")?.to_owned();
        let page = fetch(f, &url).await?;
        let policy = page
            .headers()
            .get("content-security-policy")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        assert!(policy.contains(&format!("{}{url} 'unsafe-inline'", f.origin())), "the policy is unchanged: {policy}");
        let length = page.content_length();
        let html = page.text().await?;
        assert_eq!(length, Some(html.len() as u64), "Content-Length is the rewritten length");
        assert!(html.contains(&format!("src=\"{url}assets/index-1.js\"")), "{html}");
        assert!(html.contains(&format!("href=\"{url}assets/index-1.css\"")), "{html}");
        assert_eq!(html.matches("tqa-site-report").count(), 1, "{html}");
        let css = fetch(f, &format!("{url}assets/index-1.css")).await?.text().await?;
        assert_eq!(css, format!("body{{background:url({url}assets/bg.png)}}"));
        // Scripts are never changed.
        let js = fetch(f, &format!("{url}assets/index-1.js")).await?.text().await?;
        assert_eq!(js, script);
        // A relative build's files come back as they are (the page gains only its reporter).
        let (_, minted) = mint(f, &cookie, &task, "relative/dist").await?;
        let relative = minted["url"].as_str().ok_or("url")?.to_owned();
        let html = fetch(f, &relative).await?.text().await?;
        assert!(html.contains("<script src=\"./assets/a.js\">"), "{html}");
        let css = fetch(f, &format!("{relative}assets/a.css")).await?.text().await?;
        assert_eq!(css, "body{background:url(./bg.png)}");
        // The export: its link moves under the pass, `/about` answers about.html, an unknown route the first page.
        let (_, minted) = mint(f, &cookie, &task, "export/out").await?;
        let export = minted["url"].as_str().ok_or("url")?.to_owned();
        let html = fetch(f, &export).await?.text().await?;
        assert!(html.contains(&format!("href=\"{export}about\"")), "{html}");
        assert!(fetch(f, &format!("{export}about")).await?.text().await?.contains("About us"));
        assert!(fetch(f, &format!("{export}no/such/route")).await?.text().await?.contains("href="));
        Ok(())
    })
    .await
}
