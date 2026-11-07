//! GW-U-087: an agent's HTML page is shown sandboxed. `/files/view` serves only `.html` and `.htm`, with a policy that
//! gives the page an opaque origin (no same-origin access, so no cookies or API as the person), blocks every network
//! request, form, popup and top-level navigation, and lets only our own pages frame it.
use super::*;
use axum::http::HeaderMap;

// TEST: GW-U-087
#[test]
fn gw_u_087_only_html_pages_have_a_view() -> Result<(), GatewayError> {
    for (path, viewable) in [
        ("report.html", true),
        ("out/Report.HTM", true),
        ("page.htm", true),
        ("doc.pdf", false),
        ("sheet.xlsx", false),
        ("image.svg", false),
        ("notes.md", false),
        ("script.html.js", false),
        ("html", false),
    ] {
        assert_eq!(
            viewable_path(&WorkspacePath::parse(path)?),
            viewable,
            "{path}"
        );
    }
    Ok(())
}

// TEST: GW-U-087
#[test]
fn gw_u_087_the_page_is_sandboxed_by_its_headers() {
    let mut headers = HeaderMap::new();
    sandbox(&mut headers, 1234);
    let value = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(value("content-type"), "text/html; charset=utf-8");
    assert_eq!(value("content-length"), "1234");
    assert_eq!(value("x-content-type-options"), "nosniff");
    assert_eq!(value("referrer-policy"), "no-referrer");
    assert_eq!(value("cache-control"), "no-store");
    assert_eq!(value("x-dns-prefetch-control"), "off");
    assert_eq!(value("cross-origin-resource-policy"), "same-origin");
    // Shown in the page, never saved as a download.
    assert!(value("content-disposition").is_empty());
    let policy = value("content-security-policy");
    let directives: Vec<&str> = policy.split(';').map(str::trim).collect();
    for directive in [
        "sandbox allow-scripts",
        "default-src 'none'",
        "script-src 'unsafe-inline'",
        "style-src 'unsafe-inline'",
        "img-src data:",
        "font-src data:",
        "media-src data:",
        "connect-src 'none'",
        "form-action 'none'",
        "frame-src 'none'",
        "child-src 'none'",
        "worker-src 'none'",
        "base-uri 'none'",
        "frame-ancestors 'self'",
    ] {
        assert!(
            directives.contains(&directive),
            "missing {directive:?} in {policy}"
        );
    }
    // The sandbox grants scripts only: never the page's own origin, popups, top navigation, forms or downloads.
    let sandbox = directives
        .iter()
        .find(|directive| directive.starts_with("sandbox"))
        .copied()
        .unwrap_or_default();
    assert_eq!(sandbox, "sandbox allow-scripts");
    for grant in [
        "allow-same-origin",
        "allow-popups",
        "allow-top-navigation",
        "allow-forms",
        "allow-downloads",
        "allow-modals",
    ] {
        assert!(
            !policy.contains(grant),
            "{grant} must never be granted: {policy}"
        );
    }
}
