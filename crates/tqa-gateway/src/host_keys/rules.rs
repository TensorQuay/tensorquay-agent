//! What a host key's address and name may be (ADR-0037 §1, Amendment 1; GW-U-102, GW-U-103).
use crate::InvalidReason;

/// Where a key may be used: one exact host (a name, never an address or a wildcard), a port, and an optional path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Address {
    pub host: String,
    pub port: u16,
    pub path_prefix: Option<String>,
}

/// The longest path prefix.
const PATH_BYTES: usize = 512;

/// The names a key may not take (Amendment 1): the environment a task's commands rely on, or that the runtime sets.
/// Names starting with one of [`RESERVED_PREFIXES`] are reserved too. The API documents both lists as they are here.
pub(crate) const RESERVED_NAMES: [&str; 26] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "PWD",
    "OLDPWD",
    "TMPDIR",
    "LANG",
    "LANGUAGE",
    "TERM",
    "HOSTNAME",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "PIP_CERT",
    "NODE_EXTRA_CA_CERTS",
    "NODE_OPTIONS",
    "PYTHONPATH",
    "PYTHONHOME",
    "PYTHONSTARTUP",
];
/// Name prefixes a key may not start with.
pub(crate) const RESERVED_PREFIXES: [&str; 4] = ["LC_", "LD_", "GIT_", "TQA_"];

/// The address a person typed, `https://host[:port][/path]`, as the key's host, port and path prefix; the reason it
/// can't be one otherwise.
///
/// # Errors
/// `address_not_https` for anything but an https address, `address_not_plain` for credentials, a query, a fragment or
/// a path over 512 bytes, `host_invalid` for a wildcard or no host. An IP address is an exact address in canonical form.
pub(crate) fn address(text: &str) -> Result<Address, (&'static str, InvalidReason)> {
    let refuse = |reason| Err(("address", reason));
    let Ok(url) = url::Url::parse(text.trim()) else {
        return if text.contains('*') {
            refuse(InvalidReason::HostInvalid)
        } else {
            refuse(InvalidReason::AddressNotHttps)
        };
    };
    if url.scheme() != "https" {
        return refuse(InvalidReason::AddressNotHttps);
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return refuse(InvalidReason::AddressNotPlain);
    }
    let host = match url.host() {
        Some(url::Host::Domain(host)) => {
            let host = host.to_ascii_lowercase();
            if !valid_host_name(&host) {
                return refuse(InvalidReason::HostInvalid);
            }
            host
        }
        // An exact address, used only through an allow rule for that exact address (ADR-0037 Amendment 2); canonical,
        // without brackets, as the proxy compares it.
        Some(url::Host::Ipv4(address)) => address.to_string(),
        Some(url::Host::Ipv6(address)) => address.to_string(),
        None => return refuse(InvalidReason::HostInvalid),
    };
    if url.path().trim_end_matches('/').len() > PATH_BYTES {
        return refuse(InvalidReason::AddressNotPlain);
    }
    Ok(at(host, &url))
}

/// A host name of at most 253 bytes in labels of 1 to 63 letters, digits and hyphens, none starting or ending with one.
fn valid_host_name(host: &str) -> bool {
    let label_ok = |label: &str| {
        (1..=63).contains(&label.len())
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    };
    host.len() <= 253 && host.split('.').all(label_ok)
}

/// The key's address at `host`, with the URL's port (443 unless given) and path prefix (none for `/`).
fn at(host: String, url: &url::Url) -> Address {
    let path = url.path().trim_end_matches('/');
    Address {
        port: url.port().unwrap_or(443),
        path_prefix: (!path.is_empty()).then(|| path.to_owned()),
        host,
    }
}

/// Whether `name` is a name a task may see a key by: `^[A-Z][A-Z0-9_]{0,63}$`, and not reserved.
///
/// # Errors
/// `name_invalid` or `name_reserved`.
pub(crate) fn name(name: &str) -> Result<(), InvalidReason> {
    let shaped = (1..=64).contains(&name.len())
        && name.as_bytes()[0].is_ascii_uppercase()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
    if !shaped {
        return Err(InvalidReason::NameInvalid);
    }
    if RESERVED_NAMES.contains(&name)
        || RESERVED_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
    {
        return Err(InvalidReason::NameReserved);
    }
    Ok(())
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
