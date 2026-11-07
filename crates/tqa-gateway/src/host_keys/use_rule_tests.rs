//! GW-U-106 and GW-U-110 (ADR-0037 §3, §5): where a placeholder may sit in a request, and whether a found one may be
//! used there.
use super::{Caller, Form, Outcome, Placement, judge, placement};
use crate::host_keys::placeholders::Bound;
use base64::Engine as _;
use proptest::prelude::*;
use uuid::Uuid;

const P: &str = "tqa_ph_0123456789abcdefghijklmnopqrstuvwxyzABCDEFG";
const Q: &str = "tqa_ph_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefg";

fn headers(pairs: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.as_bytes().to_vec()))
        .collect()
}

fn basic(user_and_password: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(user_and_password)
    )
}

// TEST: GW-U-106
#[test]
fn gw_u_106_a_placeholder_goes_only_in_the_authorization_header_in_its_form() {
    let bearer = format!("Bearer {P}");
    // No placeholder: forwarded as it is, whatever the headers say.
    assert_eq!(
        placement(
            "/repo.git/info/refs?service=git-upload-pack",
            &headers(&[("Accept", "*/*"), ("Authorization", "Bearer real-token")])
        ),
        Placement::None
    );
    // The two forms, exactly.
    assert_eq!(
        placement("/v1/me", &headers(&[("Authorization", &bearer)])),
        Placement::One {
            placeholder: P.to_owned(),
            form: Form::Bearer
        }
    );
    assert_eq!(
        placement(
            "/v1/me",
            &headers(&[("authorization", &format!("bearer {P}"))])
        ),
        Placement::One {
            placeholder: P.to_owned(),
            form: Form::Bearer
        },
        "the header name and scheme are case-insensitive"
    );
    assert_eq!(
        placement(
            "/acme/tools.git/info/refs",
            &headers(&[("Authorization", &basic(&format!("x-access-token:{P}")))])
        ),
        Placement::One {
            placeholder: P.to_owned(),
            form: Form::Basic {
                user: "x-access-token".into()
            }
        }
    );
}

// TEST: GW-U-106
#[test]
fn gw_u_106_a_placeholder_anywhere_else_or_in_another_shape_is_refused() {
    for (path, pairs) in misplaced() {
        let pairs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        assert_eq!(
            placement(&path, &headers(&pairs)),
            Placement::Refused,
            "{path} {pairs:?}"
        );
    }
}

/// A path and headers that put a placeholder anywhere but its one place, or in another shape.
fn misplaced() -> Vec<(String, Vec<(String, String)>)> {
    let bearer = format!("Bearer {P}");
    vec![
        (format!("/v1/me?token={P}"), vec![]),
        (format!("/v1/{P}/me"), vec![]),
        (
            "/v1/me?token=tqa%5Fph%5F0123456789abcdefghijklmnopqrstuvwxyzABCDEFG".to_owned(),
            vec![],
        ),
        (
            "/v1/me".to_owned(),
            vec![("X-Api-Key".to_owned(), P.to_owned())],
        ),
        (
            "/v1/me".to_owned(),
            vec![("Cookie".to_owned(), format!("session={P}"))],
        ),
        (
            "/v1/me".to_owned(),
            vec![("Authorization".to_owned(), format!("Token {P}"))],
        ),
        (
            "/v1/me".to_owned(),
            vec![("Authorization".to_owned(), format!("Bearer  {P}"))],
        ),
        (
            "/v1/me".to_owned(),
            vec![("Authorization".to_owned(), format!("Bearer {P} extra"))],
        ),
        (
            "/v1/me".to_owned(),
            vec![("Authorization".to_owned(), format!("Bearer x{P}"))],
        ),
        (
            "/v1/me".to_owned(),
            vec![("Authorization".to_owned(), basic(&format!("{P}:x")))],
        ),
        (
            "/v1/me".to_owned(),
            vec![("Authorization".to_owned(), basic(&format!("user:{P}x")))],
        ),
        (
            "/v1/me".to_owned(),
            vec![
                ("Authorization".to_owned(), bearer.clone()),
                ("Authorization".to_owned(), "Bearer other".to_owned()),
            ],
        ),
        (
            "/v1/me".to_owned(),
            vec![
                ("Authorization".to_owned(), bearer.clone()),
                ("X-Other".to_owned(), Q.to_owned()),
            ],
        ),
    ]
}

proptest! {
    // TEST: GW-U-106
    /// Whatever surrounds a placeholder outside the one accepted form, the request is refused, never forwarded.
    #[test]
    fn gw_u_106_a_placeholder_anywhere_else_is_refused(
        before in "[ -~]{0,24}",
        after in "[ -~]{0,24}",
        name in "[A-Za-z][A-Za-z0-9-]{0,15}",
    ) {
        let value = format!("{before}{P}{after}");
        let exact = name.eq_ignore_ascii_case("authorization")
            && (value.eq_ignore_ascii_case(&format!("bearer {P}")));
        prop_assume!(!exact);
        prop_assert_eq!(placement("/", &headers(&[(&name, &value)])), Placement::Refused);
        let path = format!("/{}", value.replace(['?', '#'], "_"));
        prop_assert_eq!(placement(&path, &[]), Placement::Refused);
    }
}

fn bound() -> Bound {
    Bound {
        key_id: Some(Uuid::nil()),
        secret_ref: Some(format!("secrets/host-{}", "0".repeat(64))),
        conversation_id: Uuid::nil(),
        sandbox_id: "sandbox-1".into(),
        run_id: Some("run-1".into()),
        ended: false,
        sender_id: Uuid::nil(),
        name: "GITHUB_WORK".into(),
        host: "github.com".into(),
        port: 443,
        path_prefix: Some("/acme".into()),
        scheme: "basic".into(),
        user_name: Some("x-access-token".into()),
    }
}

fn caller() -> Caller<'static> {
    Caller {
        conversation: Uuid::nil(),
        sandbox: "sandbox-1",
        current_run: Some("run-1"),
        host: "github.com",
        port: 443,
        path: "/acme/tools.git/info/refs",
        form: Form::Basic {
            user: "x-access-token".into(),
        },
        sender_active: true,
        allowed: true,
    }
}

/// The judgement with what the placeholder is bound to edited.
fn with(edit: fn(&mut Bound)) -> Outcome {
    let mut bound = bound();
    edit(&mut bound);
    judge(Some(&bound), &caller())
}

/// The judgement with the caller edited.
fn from(edit: fn(&mut Caller<'static>)) -> Outcome {
    let mut caller = caller();
    edit(&mut caller);
    judge(Some(&bound()), &caller)
}

// TEST: GW-U-110
#[test]
fn gw_u_110_a_placeholder_is_used_only_while_its_key_its_run_and_its_form_hold() {
    assert_eq!(judge(Some(&bound()), &caller()), Outcome::Ok);
    assert_eq!(judge(None, &caller()), Outcome::Absent);
    assert_eq!(with(|b| b.key_id = None), Outcome::Revoked);
    assert_eq!(
        with(|b| b.sandbox_id = "sandbox-2".into()),
        Outcome::ForeignSandbox
    );
    assert_eq!(
        with(|b| b.conversation_id = Uuid::max()),
        Outcome::ForeignSandbox
    );
    assert_eq!(
        with(|b| b.run_id = Some("run-0".into())),
        Outcome::ForeignRun
    );
    assert_eq!(with(|b| b.run_id = None), Outcome::ForeignRun);
    assert_eq!(with(|b| b.ended = true), Outcome::Expired);
    assert_eq!(with(|b| b.scheme = "bearer".into()), Outcome::WrongPosition);
    assert_eq!(
        with(|b| b.user_name = Some("someone".into())),
        Outcome::WrongPosition
    );
    assert_eq!(
        with(|b| b.user_name = None),
        Outcome::Ok,
        "no stored user name: any user"
    );
    assert_eq!(with(|b| b.path_prefix = None), Outcome::Ok);
}

// TEST: GW-U-110
#[test]
fn gw_u_110_a_placeholder_is_used_only_from_its_sandbox_at_its_address() {
    assert_eq!(from(|c| c.sender_active = false), Outcome::Revoked);
    assert_eq!(from(|c| c.current_run = None), Outcome::Expired);
    assert_eq!(from(|c| c.current_run = Some("run-2")), Outcome::ForeignRun);
    assert_eq!(from(|c| c.host = "gitlab.com"), Outcome::WrongHost);
    assert_eq!(from(|c| c.port = 8443), Outcome::WrongHost);
    for path in ["/acme", "/acme/", "/acme/tools.git", "/acme?x=1"] {
        let mut inside = caller();
        inside.path = path;
        assert_eq!(judge(Some(&bound()), &inside), Outcome::Ok, "{path}");
    }
    for path in [
        "/other/tools.git",
        "/acmeother/x",
        "/acme/../other",
        "/acme/./x",
        "/acme/%2e%2e/other",
        "/acme%2Fx",
        "/",
    ] {
        let mut outside = caller();
        outside.path = path;
        assert_eq!(
            judge(Some(&bound()), &outside),
            Outcome::WrongHost,
            "{path}"
        );
    }
    assert_eq!(from(|c| c.allowed = false), Outcome::NotAllowed);
    assert_eq!(Outcome::ForeignSandbox.as_str(), "foreign_sandbox");
}

// TEST: GW-U-110
/// A path some servers resolve outside the key's prefix is outside it here too (the second review's L3): a segment
/// that is `.` or `..` before a `;` parameter, as Tomcat-style servers read it, and any `%25`, which a server that
/// decodes twice turns into an escape.
#[test]
fn gw_u_110_a_path_a_server_resolves_outside_the_prefix_is_outside_it() {
    for path in [
        "/acme/..;/other",
        "/acme/..;x=1/other",
        "/acme/.;/x",
        "/acme/%2e%2e;/other",
        "/acme/%252e%252e/other",
        "/acme/%252fother",
        "/acme/x%25",
    ] {
        let mut outside = caller();
        outside.path = path;
        assert_eq!(
            judge(Some(&bound()), &outside),
            Outcome::WrongHost,
            "{path}"
        );
    }
}
