//! The proxy's rules for a keyed request (ADR-0037 §3, §5): where a placeholder may sit, and whether a found one may be
//! used there. Pure: the proxy gathers the facts and acts on the answer (G6).
use super::placeholders::Bound;
use base64::Engine as _;
use uuid::Uuid;

const MARK: &[u8] = b"tqa_ph_";
const PLACEHOLDER_BYTES: usize = 7 + 43;

/// How a request carries its placeholder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Form {
    /// `Authorization: Bearer <placeholder>`.
    Bearer,
    /// `Authorization: Basic base64(<user>:<placeholder>)`.
    Basic { user: String },
}

/// Where a request's placeholder sits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Placement {
    /// None anywhere: the request goes on as it is.
    None,
    /// Exactly one, in the one accepted place and form.
    One { placeholder: String, form: Form },
    /// Somewhere else, twice, or in another shape: refused (`wrong_position`), never forwarded.
    Refused,
}

/// Where `path` (with its query) and `headers` put a placeholder. Only the Authorization header may hold one, once,
/// in the bearer or basic form; anywhere else, percent-encoded too, refuses the request.
pub(crate) fn placement(path: &str, headers: &[(String, Vec<u8>)]) -> Placement {
    if marked(path.as_bytes()) {
        return Placement::Refused;
    }
    let authorizations = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        .count();
    let mut found = None;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("authorization") {
            match authorization(value) {
                Some(Ok(one)) if authorizations == 1 => {
                    found = Some(one);
                    continue;
                }
                Some(_) => return Placement::Refused,
                None => continue,
            }
        }
        if marked(value) {
            return Placement::Refused;
        }
    }
    found.map_or(Placement::None, |(placeholder, form)| Placement::One {
        placeholder,
        form,
    })
}

/// An Authorization value's placeholder and form: None without one, an error for one in any other shape.
fn authorization(value: &[u8]) -> Option<Result<(String, Form), ()>> {
    let Ok(text) = std::str::from_utf8(value) else {
        return marked(value).then_some(Err(()));
    };
    if let Some(token) = strip_scheme(text, "bearer ")
        && is_placeholder(token)
    {
        return Some(Ok((token.to_owned(), Form::Bearer)));
    }
    if let Some(encoded) = strip_scheme(text, "basic ") {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap_or_default();
        if let Some((user, password)) = std::str::from_utf8(&decoded)
            .ok()
            .and_then(|pair| pair.split_once(':'))
            && is_placeholder(password)
            && !marked(user.as_bytes())
        {
            return Some(Ok((
                password.to_owned(),
                Form::Basic {
                    user: user.to_owned(),
                },
            )));
        }
        if marked(&decoded) {
            return Some(Err(()));
        }
    }
    marked(value).then_some(Err(()))
}

fn strip_scheme<'a>(text: &'a str, scheme: &str) -> Option<&'a str> {
    text.get(..scheme.len())
        .filter(|start| start.eq_ignore_ascii_case(scheme))
        .map(|_| &text[scheme.len()..])
}

fn is_placeholder(text: &str) -> bool {
    text.len() == PLACEHOLDER_BYTES
        && text.as_bytes().starts_with(MARK)
        && text.as_bytes()[MARK.len()..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// Whether `bytes`, as they are or percent-decoded once or twice, hold a placeholder's mark.
pub(crate) fn marked(bytes: &[u8]) -> bool {
    let once = percent_decoded(bytes);
    contains(bytes, MARK) || contains(&once, MARK) || contains(&percent_decoded(&once), MARK)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn percent_decoded(bytes: &[u8]) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = |byte: u8| char::from(byte).to_digit(16);
        match (bytes[index], bytes.get(index + 1), bytes.get(index + 2)) {
            (b'%', Some(&high), Some(&low)) if hex(high).is_some() && hex(low).is_some() => {
                let value = hex(high).unwrap_or_default() * 16 + hex(low).unwrap_or_default();
                decoded.push(u8::try_from(value).unwrap_or_default());
                index += 3;
            }
            (byte, ..) => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    decoded
}

/// The outcome of a keyed request, as `secret_reads` records it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Outcome {
    Ok,
    Absent,
    Revoked,
    ForeignSandbox,
    ForeignRun,
    Expired,
    WrongHost,
    WrongPosition,
    NotAllowed,
    /// The use couldn't be recorded, so the value wasn't read (ADR-0037 Amendment 1).
    Unrecorded,
    /// The key's value couldn't be read from the store.
    Unavailable,
}

impl Outcome {
    /// Every outcome, for parsing.
    const ALL: [Self; 11] = [
        Self::Ok,
        Self::Absent,
        Self::Revoked,
        Self::ForeignSandbox,
        Self::ForeignRun,
        Self::Expired,
        Self::WrongHost,
        Self::WrongPosition,
        Self::NotAllowed,
        Self::Unrecorded,
        Self::Unavailable,
    ];

    /// The outcome spelled `text`, if one is.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|outcome| outcome.as_str() == text)
    }

    /// The stored and wire spelling.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Absent => "absent",
            Self::Revoked => "revoked",
            Self::ForeignSandbox => "foreign_sandbox",
            Self::ForeignRun => "foreign_run",
            Self::Expired => "expired",
            Self::WrongHost => "wrong_host",
            Self::WrongPosition => "wrong_position",
            Self::NotAllowed => "not_allowed",
            Self::Unrecorded => "unrecorded",
            Self::Unavailable => "unavailable",
        }
    }
}

/// What the proxy knows about the request carrying a placeholder.
pub(crate) struct Caller<'a> {
    /// The task and sandbox the request came from.
    pub(crate) conversation: Uuid,
    pub(crate) sandbox: &'a str,
    /// The run going in that task now, if one is.
    pub(crate) current_run: Option<&'a str>,
    /// The CONNECT's host and port, and the request's path with its query.
    pub(crate) host: &'a str,
    pub(crate) port: u16,
    pub(crate) path: &'a str,
    /// How the request carries the placeholder.
    pub(crate) form: Form,
    /// Whether the key's sender is still an active member.
    pub(crate) sender_active: bool,
    /// Whether the task's network list still allows the host and port.
    pub(crate) allowed: bool,
}

/// Whether a placeholder bound as `bound` (None: the gateway doesn't hold it) may be used by `caller`: only by the run
/// it was minted for, going now, in its sandbox, at its address, in its key's form, while the key, its sender and the
/// network list allow it.
pub(crate) fn judge(bound: Option<&Bound>, caller: &Caller<'_>) -> Outcome {
    let Some(bound) = bound else {
        return Outcome::Absent;
    };
    if bound.key_id.is_none() || !caller.sender_active {
        return Outcome::Revoked;
    }
    if bound.conversation_id != caller.conversation || bound.sandbox_id != caller.sandbox {
        return Outcome::ForeignSandbox;
    }
    match (bound.run_id.as_deref(), caller.current_run) {
        _ if bound.ended => return Outcome::Expired,
        (Some(run), Some(current)) if run == current => {}
        (Some(_), None) => return Outcome::Expired,
        _ => return Outcome::ForeignRun,
    }
    if bound.host != caller.host
        || bound.port != caller.port
        || !inside(bound.path_prefix.as_deref(), caller.path)
    {
        return Outcome::WrongHost;
    }
    let form = match (&caller.form, bound.scheme.as_str()) {
        (Form::Bearer, "bearer") => true,
        (Form::Basic { user }, "basic") => bound
            .user_name
            .as_deref()
            .is_none_or(|stored| stored == user),
        _ => false,
    };
    if !form {
        return Outcome::WrongPosition;
    }
    if !caller.allowed {
        return Outcome::NotAllowed;
    }
    Outcome::Ok
}

/// Whether `path` (its query aside) is `prefix` or below it, with no dot segment or encoded separator that a server
/// could resolve outside it.
fn inside(prefix: Option<&str>, path: &str) -> bool {
    let Some(prefix) = prefix else {
        return true;
    };
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let lowered = path.to_ascii_lowercase();
    // An encoded separator, and any `%25`, which a server that decodes twice turns into one (the second review's L3).
    if lowered.contains("%2f") || lowered.contains("%5c") || lowered.contains("%25") {
        return false;
    }
    let decoded = percent_decoded(path.as_bytes());
    // A dot segment, also before a `;` parameter, as Tomcat-style servers read `..;` (L3).
    let dotted = decoded.split(|byte| *byte == b'/').any(|segment| {
        let name = segment
            .split(|byte| *byte == b';')
            .next()
            .unwrap_or_default();
        name == b"." || name == b".."
    });
    !dotted
        && (path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/')))
}

#[cfg(test)]
#[path = "use_rule_tests.rs"]
mod tests;
