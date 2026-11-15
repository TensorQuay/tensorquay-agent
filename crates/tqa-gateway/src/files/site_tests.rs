//! GW-U-211 to GW-U-213: a workspace folder served as a site. Which file an address names (the folder's page for
//! the root, a directory and a built site's own routes), the type each file is served with, the page's policy from
//! the install's network setting, and the passes: minted per task up to a cap, renewed by use, forgotten when expired.
use super::*;
use crate::auth::Role;

fn person() -> User {
    User {
        id: uuid::Uuid::now_v7(),
        email: "person@example.invalid".into(),
        role: Role::Admin,
    }
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn conversation() -> Result<ConversationId, Box<dyn std::error::Error>> {
    Ok(uuid::Uuid::now_v7().to_string().parse()?)
}

// TEST: GW-U-211
#[test]
fn gw_u_211_an_address_names_the_folders_file_or_its_page() -> TestResult {
    let page = WorkspacePath::parse("dist/index.html")?;
    for (rest, expected) in [
        ("", (page.clone(), true)),
        ("/", (page.clone(), true)),
        ("index.html", (page.clone(), true)),
        ("about", (page.clone(), true)),
        ("docs/", (page.clone(), true)),
        ("docs/getting-started", (page.clone(), true)),
        (
            "assets/app.js",
            (WorkspacePath::parse("dist/assets/app.js")?, false),
        ),
        (
            "style.css",
            (WorkspacePath::parse("dist/style.css")?, false),
        ),
        (
            "/favicon.ico",
            (WorkspacePath::parse("dist/favicon.ico")?, false),
        ),
    ] {
        assert_eq!(target_for("dist", rest), Some(expected), "{rest:?}");
    }
    // A path that climbs, a private area or a link is refused as missing: nothing says which.
    for rest in ["../secrets.txt", "a/../../x.js", ".tqa/notes.txt"] {
        assert_eq!(target_for("dist", rest), None, "{rest:?}");
    }
    // The workspace root as the folder.
    assert_eq!(
        target_for("", "app.js"),
        Some((WorkspacePath::parse("app.js")?, false))
    );
    assert_eq!(
        target_for("", ""),
        Some((WorkspacePath::parse("index.html")?, true))
    );
    Ok(())
}

// TEST: GW-U-211
#[test]
fn gw_u_211_a_file_is_served_by_its_extensions_type_never_guessed() -> TestResult {
    for (name, kind) in [
        ("index.html", "text/html; charset=utf-8"),
        ("Page.HTM", "text/html; charset=utf-8"),
        ("app.js", "text/javascript; charset=utf-8"),
        ("chunk.mjs", "text/javascript; charset=utf-8"),
        ("style.css", "text/css; charset=utf-8"),
        ("data.json", "application/json"),
        ("app.js.map", "application/json"),
        ("logo.svg", "image/svg+xml"),
        ("a.png", "image/png"),
        ("b.jpeg", "image/jpeg"),
        ("c.webp", "image/webp"),
        ("favicon.ico", "image/x-icon"),
        ("f.woff2", "font/woff2"),
        ("m.wasm", "application/wasm"),
        ("notes.txt", "text/plain; charset=utf-8"),
        ("archive.tar.gz", "application/octet-stream"),
        ("Makefile", "application/octet-stream"),
        ("page.html.exe", "application/octet-stream"),
    ] {
        assert_eq!(content_type(&WorkspacePath::parse(name)?), kind, "{name}");
    }
    Ok(())
}

// TEST: GW-U-212
#[test]
fn gw_u_212_the_pages_policy_names_its_own_root_and_the_network_setting() {
    let own = "http://localhost:8080/api/v1/conversations/c/site/p/";
    let closed = policy(own, &[]);
    assert!(closed.starts_with("sandbox allow-scripts; default-src 'none'; "));
    assert!(!closed.contains("allow-same-origin"));
    assert!(closed.contains(&format!(
        "script-src {own} 'unsafe-inline'; style-src {own} 'unsafe-inline'; "
    )));
    assert!(closed.contains(&format!(
        "img-src {own} data: blob:; font-src {own} data:; media-src {own} data: blob:; "
    )));
    assert!(closed.contains(&format!("connect-src {own}; ")));
    assert!(closed.ends_with(
        "form-action 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; base-uri 'none'; frame-ancestors 'self'"
    ));
    assert!(
        !closed.contains("https"),
        "nothing outside without a rule: {closed}"
    );
    let open = policy(
        own,
        &[
            "https://cdn.example.org".to_owned(),
            "https://*.fonts.example".to_owned(),
        ],
    );
    for directive in [
        "script-src",
        "style-src",
        "img-src",
        "font-src",
        "media-src",
        "connect-src",
    ] {
        let value = open
            .split("; ")
            .find(|part| part.starts_with(directive))
            .unwrap_or_default();
        assert!(
            value.ends_with("https://cdn.example.org https://*.fonts.example"),
            "{directive}: {value}"
        );
    }
    let any = policy(own, &["https:".to_owned()]);
    assert!(
        any.contains("connect-src http://localhost:8080/api/v1/conversations/c/site/p/ https:; ")
    );
}

// TEST: GW-U-213
#[test]
fn gw_u_213_passes_are_minted_per_task_up_to_the_cap_renewed_by_use_and_forgotten_when_expired()
-> TestResult {
    let passes = Passes::default();
    let (task, other) = (conversation()?, conversation()?);
    let person = person();
    let folder = WorkspacePath::parse("dist")?;
    let start = Instant::now();
    let mut minted = Vec::new();
    for _ in 0..PASSES_PER_TASK {
        minted.push(passes.mint(start, &task, &person, folder.clone())?);
    }
    assert!(
        minted.iter().all(|pass| pass.len() == 43),
        "unpadded base64url of 32 bytes"
    );
    assert_eq!(
        minted
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        minted.len()
    );
    assert!(matches!(
        passes.mint(start, &task, &person, folder.clone()),
        Err(GatewayError::Conflict("site_passes_limit"))
    ));
    // Another task has its own cap.
    passes.mint(start, &other, &person, folder.clone())?;
    // A pass serves its own task only, and nothing says a mismatch from an unknown pass.
    let later = start + Duration::from_secs(PASS_SECONDS - 1);
    assert!(passes.take(later, &task, &minted[0]).is_some());
    assert!(passes.take(later, &other, &minted[0]).is_none());
    assert!(passes.take(later, &task, "no-such-pass").is_none());
    // A use renews: the first pass, used at the last second, lives on past the original expiry; the untouched ones go.
    let past = start + Duration::from_secs(PASS_SECONDS);
    assert!(
        passes.take(past, &task, &minted[0]).is_some(),
        "renewed by its use"
    );
    assert!(
        passes.take(past, &task, &minted[1]).is_none(),
        "expired, untouched"
    );
    assert_eq!(passes.live(past, &task), 1);
    // Expired passes make room for new ones.
    for _ in 0..(PASSES_PER_TASK - 1) {
        passes.mint(past, &task, &person, folder.clone())?;
    }
    assert!(matches!(
        passes.mint(past, &task, &person, folder),
        Err(GatewayError::Conflict("site_passes_limit"))
    ));
    Ok(())
}
