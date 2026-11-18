//! GW-U-221: root paths in what a site pass serves are moved under the pass (18.1, built web apps preview as they
//! are); scripts never change. GW-U-222: the reporter goes first in each page's head, once.
use super::*;

const ROOT: &str = "/api/v1/conversations/c1/site/p1/";

fn page(html: &str) -> String {
    String::from_utf8(html_under_root(html.as_bytes(), ROOT, "<script>R</script>"))
        .unwrap_or_default()
}

// TEST: GW-U-221
#[test]
fn gw_u_221_the_root_path_rule() {
    for (url, moved) in [
        ("/x", Some("/api/v1/conversations/c1/site/p1/x")),
        ("/", Some("/api/v1/conversations/c1/site/p1/")),
        (
            "/assets/a.js?v=2#top",
            Some("/api/v1/conversations/c1/site/p1/assets/a.js?v=2#top"),
        ),
        ("/../../x", Some("/api/v1/conversations/c1/site/p1/x")),
        ("/a/./b", Some("/api/v1/conversations/c1/site/p1/a/b")),
        ("/a/b/..", Some("/api/v1/conversations/c1/site/p1/a/")),
        (
            "/a%20b/c%2Fd",
            Some("/api/v1/conversations/c1/site/p1/a%20b/c%2Fd"),
        ),
        ("  /x  ", Some("/api/v1/conversations/c1/site/p1/x")),
        ("//cdn.example.org/x", None),
        ("/\\cdn.example.org/x", None),
        ("\\x", None),
        ("x", None),
        ("./x", None),
        ("../x", None),
        ("#a", None),
        ("data:text/plain,a", None),
        ("https://example.org/x", None),
        ("mailto:a@example.org", None),
        ("javascript:void(0)", None),
        ("", None),
    ] {
        assert_eq!(under_root(url, ROOT).as_deref(), moved, "{url:?}");
    }
}

// TEST: GW-U-221
#[test]
fn gw_u_221_every_attribute_of_the_table() {
    let html = page(
        r#"<html><head><script type="module" src="/assets/a.js"></script><link rel="stylesheet" href="/assets/a.css"><link rel="preload" as="image" imagesrcset="/i/a.png 1x, /i/b.png 2x"></head><body><img src="/logo.png" srcset="/l1.png 1x,/l2.png 2x, ./l3.png 3x"><picture><source src="/s.webm" srcset="/s.png"></picture><video src="/v.mp4" poster="/p.png"><track src="/t.vtt"></video><audio src="/a.mp3"></audio><a href="/about">About</a><map><area href="/area"></map><svg><image href="/svg.png"/><use xlink:href="/sprite.svg#i"/></svg></body></html>"#,
    );
    for moved in [
        r#"src="/api/v1/conversations/c1/site/p1/assets/a.js""#,
        r#"href="/api/v1/conversations/c1/site/p1/assets/a.css""#,
        r#"imagesrcset="/api/v1/conversations/c1/site/p1/i/a.png 1x, /api/v1/conversations/c1/site/p1/i/b.png 2x""#,
        r#"src="/api/v1/conversations/c1/site/p1/logo.png""#,
        r#"srcset="/api/v1/conversations/c1/site/p1/l1.png 1x,/api/v1/conversations/c1/site/p1/l2.png 2x, ./l3.png 3x""#,
        r#"src="/api/v1/conversations/c1/site/p1/s.webm""#,
        r#"srcset="/api/v1/conversations/c1/site/p1/s.png""#,
        r#"src="/api/v1/conversations/c1/site/p1/v.mp4""#,
        r#"poster="/api/v1/conversations/c1/site/p1/p.png""#,
        r#"src="/api/v1/conversations/c1/site/p1/t.vtt""#,
        r#"src="/api/v1/conversations/c1/site/p1/a.mp3""#,
        r#"href="/api/v1/conversations/c1/site/p1/about""#,
        r#"href="/api/v1/conversations/c1/site/p1/area""#,
        r#"href="/api/v1/conversations/c1/site/p1/svg.png""#,
        r#"xlink:href="/api/v1/conversations/c1/site/p1/sprite.svg#i""#,
    ] {
        assert!(html.contains(moved), "{moved}\n{html}");
    }
}

// TEST: GW-U-221
#[test]
fn gw_u_221_style_is_moved_and_scripts_are_never_changed() {
    let html = page(
        r#"<head><style>body{background:url(/bg.png)} @import "/more.css";</style><script>const a = "/assets/logo.png"; fetch("/data.json")</script><script type="importmap">{"imports":{"x":"/x.js"}}</script></head><body><div style="background: url('/d.png')" onclick="go('/x')"></div><p>/not/a/path</p></body>"#,
    );
    assert!(
        html.contains("url(/api/v1/conversations/c1/site/p1/bg.png)"),
        "{html}"
    );
    assert!(
        html.contains(r#"@import "/api/v1/conversations/c1/site/p1/more.css""#),
        "{html}"
    );
    assert!(
        html.contains("url('/api/v1/conversations/c1/site/p1/d.png')"),
        "{html}"
    );
    assert!(
        html.contains(r#"const a = "/assets/logo.png"; fetch("/data.json")"#),
        "{html}"
    );
    assert!(html.contains(r#"{"imports":{"x":"/x.js"}}"#), "{html}");
    assert!(html.contains(r#"onclick="go('/x')""#), "{html}");
    assert!(html.contains("<p>/not/a/path</p>"), "{html}");
}

// TEST: GW-U-221
#[test]
fn gw_u_221_odd_case_and_quoting() {
    let html = page("<HEAD><SCRIPT SRC=/a.js></SCRIPT><LINK HREF='/b.css' REL=stylesheet></HEAD>");
    assert!(
        html.contains("/api/v1/conversations/c1/site/p1/a.js"),
        "{html}"
    );
    assert!(
        html.contains("/api/v1/conversations/c1/site/p1/b.css"),
        "{html}"
    );
}

// TEST: GW-U-221
#[test]
fn gw_u_221_css() {
    for (css, moved) in [
        (
            "a{b:url(/x.png)}",
            Some("a{b:url(/api/v1/conversations/c1/site/p1/x.png)}"),
        ),
        (
            "a{b:url( '/x.png' )}",
            Some("a{b:url( '/api/v1/conversations/c1/site/p1/x.png' )}"),
        ),
        (
            r#"a{b:URL("/x.png")}"#,
            Some(r#"a{b:URL("/api/v1/conversations/c1/site/p1/x.png")}"#),
        ),
        (
            "@import '/a.css';",
            Some("@import '/api/v1/conversations/c1/site/p1/a.css';"),
        ),
        (
            "@import url(/a.css) screen;",
            Some("@import url(/api/v1/conversations/c1/site/p1/a.css) screen;"),
        ),
        ("/* url(/x.png) */ a{}", None),
        (r#"a{content:"url(/x.png)"}"#, None),
        (r"a{b:url(\2f x.png)}", None),
        ("a{b:url(x.png) c:url(//h/x.png)}", None),
        ("a{b:myurl(/x.png)}", None),
        ("", None),
    ] {
        assert_eq!(css_under_root(css, ROOT).as_deref(), moved, "{css:?}");
    }
}

// TEST: GW-U-221
#[test]
fn gw_u_221_srcset_keeps_descriptors_and_separators() {
    assert_eq!(
        srcset_under_root("/a.png 480w, /b.png 800w,/c.png", ROOT).as_deref(),
        Some(
            "/api/v1/conversations/c1/site/p1/a.png 480w, /api/v1/conversations/c1/site/p1/b.png 800w,\
             /api/v1/conversations/c1/site/p1/c.png"
        )
    );
    assert_eq!(srcset_under_root("a.png 1x, b.png 2x", ROOT), None);
}

// TEST: GW-U-222
#[test]
fn gw_u_222_the_reporter_goes_first_in_the_head_once() {
    let with_head =
        page("<!doctype html><html><head><title>t</title></head><body><p>x</p></body></html>");
    assert!(
        with_head.starts_with("<!doctype html><html><head><script>R</script><title>"),
        "{with_head}"
    );
    assert_eq!(with_head.matches("<script>R</script>").count(), 1);
    let without_head = page("<!doctype html><title>t</title><p>x</p>");
    assert!(
        without_head.starts_with("<!doctype html><script>R</script><title>"),
        "{without_head}"
    );
    let body_only = page("<html><body><p>x</p></body></html>");
    assert!(
        body_only.starts_with("<html><script>R</script><body>"),
        "{body_only}"
    );
    assert_eq!(page(""), "<script>R</script>");
}

// TEST: GW-U-222
#[test]
fn gw_u_222_the_reporter_names_the_pass_and_the_panel_origin() {
    let script = reporter(ROOT, "https://agent.example.org");
    assert!(script.starts_with("<script>") && script.ends_with("</script>"));
    assert!(
        script.contains(r#"B="/api/v1/conversations/c1/site/p1/""#),
        "{script}"
    );
    assert!(
        script.contains(r#"O="https://agent.example.org""#),
        "{script}"
    );
    assert!(script.contains("tqa-site-report"));
    // A pass root can't close the script early.
    assert!(!reporter("/x</script>/", "https://a").contains("x</script>/"));
}
