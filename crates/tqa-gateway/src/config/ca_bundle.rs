//! The certificates `[egress] ca_file` names: trusted beside the public roots for keyed hosts behind an internal
//! certificate authority (ADR-0037 §4). The file is read
//! once at start, bounded, and must hold certificate sections only, each an authority (basicConstraints `cA` true).
//! There is no certificate parser here (ADR-0037 §6: rcgen without one), so the authority check walks only the DER the
//! RFC 5280 path to that one field needs, bounded by the input, and treats anything unexpected as malformed.
use crate::GatewayError;
use std::{io::Read as _, path::Path, sync::Arc};

/// The largest accepted CA bundle.
const MAX_CA_BYTES: u64 = 1024 * 1024;
/// basicConstraints: 2.5.29.19.
const BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1d, 0x13];

/// Reads a bundle of authority certificates (`egress.ca_file`), or stops the start with `message`: at most 1 MiB (read
/// bounded, whatever the file grows to meanwhile), PEM certificate sections only (a key, a CRL or any other section is
/// refused, as is no certificate), and each certificate an authority.
///
/// # Errors
/// Any of the above, with `message`.
pub(super) fn read(path: &Path, message: &'static str) -> Result<Arc<Vec<u8>>, GatewayError> {
    let pem = bounded(path).ok_or(GatewayError::Configuration(message))?;
    if !strict(&pem) {
        return Err(GatewayError::Configuration(message));
    }
    Ok(Arc::new(pem))
}

/// The file's bytes, read through a 1 MiB bound; `None` when unreadable or over it.
fn bounded(path: &Path) -> Option<Vec<u8>> {
    let mut pem = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_CA_BYTES + 1).read_to_end(&mut pem))
        .ok()?;
    (pem.len() as u64 <= MAX_CA_BYTES).then_some(pem)
}

/// The strict rule: certificate sections only, each an authority, and both the gateway's HTTP client (reqwest) and the proxy's
/// (rustls) accept every one, so neither refuses it later.
fn strict(pem: &[u8]) -> bool {
    only_certificates(pem)
        && reqwest::Certificate::from_pem_bundle(pem)
            .is_ok_and(|certificates| !certificates.is_empty())
        && add_roots(&mut rustls::RootCertStore::empty(), pem).is_some()
}

/// Every PEM section is a certificate, at least one, and each is an authority. Text between sections is ignored, as PEM
/// allows; a section the parser would skip (an unknown or non-certificate label) is caught by counting the labels.
fn only_certificates(pem: &[u8]) -> bool {
    use rustls::pki_types::{pem::PemObject, pem::SectionKind};
    let Ok(text) = std::str::from_utf8(pem) else {
        return false;
    };
    let labels = text
        .lines()
        .filter(|line| line.trim_start().starts_with("-----BEGIN "))
        .count();
    let mut certificates = 0_usize;
    for section in <(SectionKind, Vec<u8>)>::pem_slice_iter(pem) {
        match section {
            Ok((SectionKind::Certificate, der)) if authority(&der) == Some(true) => {
                certificates += 1;
            }
            _ => return false,
        }
    }
    certificates > 0 && certificates == labels
}

/// The bundle's certificates added to `store` as roots; `None` when one doesn't parse or isn't a usable root.
pub(super) fn add_roots(store: &mut rustls::RootCertStore, pem: &[u8]) -> Option<()> {
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    let mut added = 0_usize;
    for certificate in CertificateDer::pem_slice_iter(pem) {
        store.add(certificate.ok()?).ok()?;
        added += 1;
    }
    (added > 0).then_some(())
}

/// One DER element: its tag, its contents and what follows it. Single-byte tags and definite lengths of at most four
/// bytes only (DER, and all a certificate's path to basicConstraints uses); anything else is malformed.
fn element(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    if tag & 0x1f == 0x1f {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (bytes, rest) = rest.split_at(count);
        (
            bytes
                .iter()
                .fold(0_usize, |sum, byte| (sum << 8) | usize::from(*byte)),
            rest,
        )
    };
    (rest.len() >= length).then(|| (tag, &rest[..length], &rest[length..]))
}

/// The elements of a constructed value's contents, in order; `None` if any is malformed.
fn elements(mut input: &[u8]) -> Option<Vec<(u8, &[u8])>> {
    let mut found = Vec::new();
    while !input.is_empty() {
        let (tag, contents, rest) = element(input)?;
        found.push((tag, contents));
        input = rest;
    }
    Some(found)
}

/// Whether the certificate is an authority: its basicConstraints extension says `cA` TRUE. `Some(false)` without the
/// extension (a v1 certificate included) or with `cA` absent or false; `None` when the DER on that path is malformed or
/// the extension appears twice.
pub(super) fn authority(der: &[u8]) -> Option<bool> {
    const SEQUENCE: u8 = 0x30;
    let (tag, certificate, rest) = element(der)?;
    if tag != SEQUENCE || !rest.is_empty() {
        return None;
    }
    let (tag, tbs, _) = element(certificate)?;
    if tag != SEQUENCE {
        return None;
    }
    // tbsCertificate's extensions are its [3] EXPLICIT field, a SEQUENCE of Extension.
    let Some((_, explicit)) = elements(tbs)?.into_iter().find(|(tag, _)| *tag == 0xa3) else {
        return Some(false);
    };
    let (tag, list, rest) = element(explicit)?;
    if tag != SEQUENCE || !rest.is_empty() {
        return None;
    }
    let mut answer = None;
    for (tag, extension) in elements(list)? {
        let fields = elements(extension)?;
        // extnID, critical BOOLEAN (optional), extnValue OCTET STRING holding BasicConstraints ::= SEQUENCE.
        let (tag, value) = match fields.as_slice() {
            _ if tag != SEQUENCE => continue,
            [] => return None,
            [(0x06, BASIC_CONSTRAINTS), .., extn_value] => *extn_value,
            _ => continue,
        };
        if answer.is_some() {
            return None;
        }
        let (inner, constraints, rest) = element(value)?;
        if tag != 0x04 || inner != SEQUENCE || !rest.is_empty() {
            return None;
        }
        let ca = matches!(elements(constraints)?.first(), Some((0x01, [0xff])));
        answer = Some(ca);
    }
    Some(answer.unwrap_or(false))
}

#[cfg(test)]
#[path = "ca_bundle_tests.rs"]
mod tests;
