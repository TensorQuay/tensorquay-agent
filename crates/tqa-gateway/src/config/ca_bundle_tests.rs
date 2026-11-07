//! GW-U-114: the authority check reads basicConstraints from the DER without a certificate parser, and treats anything
//! unexpected on that path as malformed.
use super::authority;

fn certificate(
    ca: rcgen::IsCa,
    extra: Option<rcgen::CustomExtension>,
) -> Result<Vec<u8>, rcgen::Error> {
    let key = rcgen::KeyPair::generate()?;
    let mut params = rcgen::CertificateParams::new(vec!["internal.example".to_owned()])?;
    params.is_ca = ca;
    params.custom_extensions.extend(extra);
    Ok(params.self_signed(&key)?.der().to_vec())
}

// TEST: GW-U-114
#[test]
fn gw_u_114_only_an_authority_is_an_authority() -> Result<(), Box<dyn std::error::Error>> {
    let unconstrained = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    assert_eq!(authority(&certificate(unconstrained, None)?), Some(true));
    assert_eq!(
        authority(&certificate(
            rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0)),
            None
        )?),
        Some(true)
    );
    // A server's own certificate: none, and an explicit cA false.
    assert_eq!(
        authority(&certificate(rcgen::IsCa::NoCa, None)?),
        Some(false)
    );
    assert_eq!(
        authority(&certificate(rcgen::IsCa::ExplicitNoCa, None)?),
        Some(false)
    );
    Ok(())
}

// TEST: GW-U-114
#[test]
fn gw_u_114_a_repeated_or_damaged_certificate_is_malformed()
-> Result<(), Box<dyn std::error::Error>> {
    // basicConstraints twice (cA true again): which one counts is ambiguous, so neither does.
    let again = rcgen::CustomExtension::from_oid_content(
        &[2, 5, 29, 19],
        vec![0x30, 0x03, 0x01, 0x01, 0xff],
    );
    let unconstrained = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    assert_eq!(authority(&certificate(unconstrained, Some(again))?), None);
    // Every truncation of a real authority's certificate, and a byte after it: never an authority, never a panic.
    let whole = certificate(unconstrained, None)?;
    for end in 0..whole.len() {
        assert_ne!(authority(&whole[..end]), Some(true), "{end} bytes");
    }
    let mut longer = whole.clone();
    longer.push(0);
    assert_eq!(authority(&longer), None);
    // A long-form length longer than four bytes, and an indefinite one.
    assert_eq!(authority(&[0x30, 0x85, 1, 0, 0, 0, 0]), None);
    assert_eq!(authority(&[0x30, 0x80, 0, 0]), None);
    Ok(())
}

/// One DER element.
fn tlv(tag: u8, contents: &[u8]) -> Vec<u8> {
    let length = contents.len();
    let mut out = vec![tag];
    if length < 0x80 {
        out.push(u8::try_from(length).unwrap_or(0));
    } else {
        out.push(0x82);
        out.extend_from_slice(&u16::try_from(length).unwrap_or(0).to_be_bytes());
    }
    out.extend_from_slice(contents);
    out
}

/// A certificate's shape around `extensions` (the contents of the [3] field), or without one.
fn shaped(extensions: Option<&[u8]>) -> Vec<u8> {
    let mut tbs = tlv(0x02, &[1]);
    if let Some(extensions) = extensions {
        tbs.extend(tlv(0xa3, extensions));
    }
    let mut certificate = tlv(0x30, &tbs);
    certificate.extend(tlv(0x30, &[]));
    tlv(0x30, &certificate)
}

/// basicConstraints with `value` as its extnValue's contents, and `critical` before it.
fn basic(critical: bool, value: &[u8]) -> Vec<u8> {
    let mut extension = tlv(0x06, &[0x55, 0x1d, 0x13]);
    if critical {
        extension.extend(tlv(0x01, &[0xff]));
    }
    extension.extend(tlv(0x04, value));
    tlv(0x30, &extension)
}

// TEST: GW-U-114
#[test]
fn gw_u_114_every_turn_of_the_der_walk() {
    let ca_true = tlv(0x30, &tlv(0x01, &[0xff]));
    let one = |extension: Vec<u8>| shaped(Some(&tlv(0x30, &extension)));
    // The shapes that answer.
    assert_eq!(authority(&one(basic(true, &ca_true))), Some(true));
    assert_eq!(authority(&one(basic(false, &ca_true))), Some(true));
    assert_eq!(
        authority(&one(basic(false, &tlv(0x30, &[])))),
        Some(false),
        "cA absent"
    );
    assert_eq!(
        authority(&one(basic(false, &tlv(0x30, &tlv(0x01, &[0x01]))))),
        Some(false),
        "not DER true"
    );
    assert_eq!(authority(&shaped(None)), Some(false), "no extensions (v1)");
    let other = tlv(
        0x30,
        &[tlv(0x06, &[0x55, 0x1d, 0x0f]), tlv(0x04, &[0])].concat(),
    );
    assert_eq!(
        authority(&one(other)),
        Some(false),
        "another extension only"
    );
    // Malformed on the path, each turn.
    for (bytes, what) in [
        (tlv(0x02, &[]), "not a SEQUENCE"),
        (tlv(0x30, &tlv(0x02, &[])), "tbs not a SEQUENCE"),
        (shaped(Some(&[])), "an empty extensions field"),
        (one(basic(false, &[])), "an empty extnValue"),
        (shaped(Some(&tlv(0x31, &[]))), "extensions not a SEQUENCE"),
        (
            shaped(Some(&[tlv(0x30, &[]), vec![0]].concat())),
            "after the extensions",
        ),
        (
            shaped(Some(&tlv(0x30, &tlv(0x30, &[])))),
            "an empty extension",
        ),
        (
            shaped(Some(&tlv(0x30, &[0x30, 0x05]))),
            "an extension cut short",
        ),
        (
            one(basic(false, &tlv(0x31, &[]))),
            "BasicConstraints not a SEQUENCE",
        ),
        (
            one([tlv(0x06, &[0x55, 0x1d, 0x13]), tlv(0x05, &ca_true)].concat()).clone(),
            "extnValue not an OCTET STRING",
        ),
        (
            one(basic(false, &[ca_true.clone(), vec![0]].concat())),
            "after BasicConstraints",
        ),
        (one(basic(false, &tlv(0x30, &[0x01]))), "a cut cA"),
        (vec![0x1f, 0x00], "a high tag number"),
        (vec![0x30, 0x82, 0x01], "a length cut short"),
        (vec![0x30, 0x05, 0x00], "contents cut short"),
        (vec![0x30], "no length"),
        (vec![], "nothing"),
    ] {
        assert_eq!(authority(&bytes), None, "{what}");
    }
}

proptest::proptest! {
    // TEST: GW-U-114
    /// Arbitrary bytes, and a real authority with bytes changed: never a panic. Each call walks the input once, so the
    /// bound is the input itself (at most the file's 1 MiB).
    #[test]
    fn gw_u_114_any_bytes_are_walked_without_a_panic(
        bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512),
        edits in proptest::collection::vec((0usize..4096, proptest::num::u8::ANY), 0..8),
    ) {
        let _ = authority(&bytes);
        let mut real = certificate(rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained), None)
            .map_err(|error| proptest::test_runner::TestCaseError::fail(error.to_string()))?;
        for (at, byte) in edits {
            let at = at % real.len();
            real[at] = byte;
        }
        let _ = authority(&real);
        let _ = super::only_certificates(&bytes);
    }

    // TEST: GW-U-114
    /// PEM armour around arbitrary bytes: the section check never panics, and never accepts what isn't an authority.
    #[test]
    fn gw_u_114_any_pem_is_judged_without_a_panic(
        body in proptest::collection::vec(proptest::num::u8::ANY, 0..256),
        label in "[A-Z ]{1,16}",
    ) {
        use base64::Engine as _;
        let text = format!(
            "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
            base64::engine::general_purpose::STANDARD.encode(&body)
        );
        if super::only_certificates(text.as_bytes()) {
            proptest::prop_assert_eq!(authority(&body), Some(true));
        }
        let _ = super::strict(text.as_bytes());
    }
}

// TEST: GW-U-114
#[test]
fn gw_u_114_the_readers_refuse_and_accept_by_their_rules() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    let write = |name: &str, bytes: &[u8]| -> std::io::Result<std::path::PathBuf> {
        let path = directory.path().join(name);
        std::fs::write(&path, bytes)?;
        Ok(path)
    };
    let key = rcgen::KeyPair::generate()?;
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new())?;
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let authority_pem = params.self_signed(&key)?.pem();
    let good = write("good.pem", authority_pem.as_bytes())?;
    let not_utf8 = write("binary.pem", &[0xff, 0xfe, 0x00])?;
    let large = write("large.pem", &vec![b'a'; 1024 * 1024 + 1])?;
    let missing = directory.path().join("absent.pem");
    assert!(super::read(&good, "m").is_ok());
    for path in [&not_utf8, &large, &missing] {
        assert!(super::read(path, "m").is_err(), "{}", path.display());
    }
    // The bound is exactly 1 MiB, read bounded: a good file padded (text outside sections, which PEM allows) to 1 MiB
    // loads; one byte more is refused, though its first 1 MiB is the same good file.
    let mut padded = authority_pem.clone().into_bytes();
    while padded.len() < 1024 * 1024 {
        let room = 1024 * 1024 - padded.len();
        padded.extend(std::iter::repeat_n(b'#', room.min(63)));
        padded.push(b'\n');
    }
    padded.truncate(1024 * 1024);
    let at_bound = write("at-bound.pem", &padded)?;
    assert!(super::read(&at_bound, "m").is_ok());
    padded.push(b'#');
    let over = write("over.pem", &padded)?;
    assert!(super::read(&over, "m").is_err());
    // A certificate section whose text isn't base64.
    let armour = |body: &str| {
        format!(
            "{}\n{body}\n{}\n",
            ["-----BEGIN", "CERTIFICATE-----"].join(" "),
            ["-----END", "CERTIFICATE-----"].join(" ")
        )
    };
    let garbled = write("garbled.pem", armour("!!!! not base64 !!!!").as_bytes())?;
    assert!(super::read(&garbled, "m").is_err());
    // add_roots: nothing, a section that doesn't decode, or a certificate section that isn't DER.
    let mut store = rustls::RootCertStore::empty();
    assert!(super::add_roots(&mut store, b"").is_none());
    assert!(super::add_roots(&mut store, armour("!!!!").as_bytes()).is_none());
    let damaged = authority_pem.replace("\nM", "\nZ");
    assert!(super::add_roots(&mut store, damaged.as_bytes()).is_none());
    Ok(())
}
