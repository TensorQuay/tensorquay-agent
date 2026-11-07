// TEST: SBX-U-010
use std::error::Error;

use super::{
    Bound, Bounds, HEADER_BYTES, LayerScan, MAX_LISTED, MAX_LISTED_PATH_BYTES, Rule, ScanError,
    SecretFile, SecretFiles,
};
use std::collections::{BTreeMap, BTreeSet};

type TestResult = Result<(), Box<dyn Error>>;

/// One entry of a fixture layer.
enum Entry<'a> {
    File(&'a str, Vec<u8>),
    Dir(&'a str),
    Symlink(&'a str, &'a str),
    HardLink(&'a str, &'a str),
}

use Entry::{Dir, File, HardLink, Symlink};

fn layer(entries: &[Entry<'_>]) -> std::io::Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        match entry {
            File(path, data) => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(data.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, path, data.as_slice())?;
            }
            Dir(path) => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
                header.set_cksum();
                builder.append_data(&mut header, path, &[][..])?;
            }
            Symlink(path, target) | HardLink(path, target) => {
                let kind = if matches!(entry, Symlink(..)) {
                    tar::EntryType::Symlink
                } else {
                    tar::EntryType::Link
                };
                header.set_entry_type(kind);
                header.set_size(0);
                header.set_mode(0o777);
                builder.append_link(&mut header, path, target)?;
            }
        }
    }
    builder.into_inner()
}

fn scan(layers: &[Vec<u8>]) -> Result<SecretFiles, ScanError> {
    scan_bounded(layers, Bounds::default())
}

fn scan_bounded(layers: &[Vec<u8>], bounds: Bounds) -> Result<SecretFiles, ScanError> {
    let mut scan = LayerScan::new(bounds);
    for data in layers {
        scan.layer(data.as_slice())?;
    }
    Ok(scan.finish())
}

fn paths(found: &SecretFiles) -> Vec<&str> {
    found.paths()
}

/// A private key's shape, assembled here so the repository's own scanners never see a key-like literal.
fn key() -> Vec<u8> {
    pem("OPENSSH PRIVATE KEY")
}

fn pem(kind: &str) -> Vec<u8> {
    format!("-----BEGIN {kind}-----\nMIIBOgIBAAJBAKj34GkxFhD90vcN\n-----END {kind}-----\n")
        .into_bytes()
}

#[test]
fn every_credential_shape_is_one_hit_naming_its_path() -> TestResult {
    let found = scan(&[layer(&[
        File("srv/app/server.key", pem("RSA PRIVATE KEY")),
        File("srv/app/plain.key", pem("PRIVATE KEY")),
        File("srv/app/ec.key", pem("EC PRIVATE KEY")),
        File("srv/app/signing.asc", pem("PGP PRIVATE KEY BLOCK")),
        File(
            "root/.git-credentials",
            b"https://user:token@git.example.invalid\n".to_vec(),
        ),
        File(
            "root/.netrc",
            b"machine git.example.invalid login user password hunter2\n".to_vec(),
        ),
        File(
            "root/.aws/credentials",
            b"[default]\naws_access_key_id = x\naws_secret_access_key = y\n".to_vec(),
        ),
        File(
            "root/.docker/config.json",
            br#"{"auths":{"registry.example.invalid":{"auth":"dXNlcjpwYXNz"}}}"#.to_vec(),
        ),
        File(
            "var/lib/app/.npmrc",
            b"//registry.npmjs.org/:_authToken=npm_x\n".to_vec(),
        ),
        File(
            "var/lib/app/.pypirc",
            b"[pypi]\nusername = __token__\npassword = pypi-x\n".to_vec(),
        ),
        File(
            "var/lib/app/.ssh/id_ed25519",
            b"not even a key body".to_vec(),
        ),
    ])?])?;
    assert_eq!(
        paths(&found),
        [
            "/root/.aws/credentials",
            "/root/.docker/config.json",
            "/root/.git-credentials",
            "/root/.netrc",
            "/srv/app/ec.key",
            "/srv/app/plain.key",
            "/srv/app/server.key",
            "/srv/app/signing.asc",
            "/var/lib/app/.npmrc",
            "/var/lib/app/.pypirc",
            "/var/lib/app/.ssh/id_ed25519",
        ]
    );
    assert_eq!(found.total, 11);
    Ok(())
}

#[test]
fn public_material_links_and_rule_misses_are_not_hits() -> TestResult {
    let found = scan(&[layer(&[
        File("etc/ssl/certs/ca.pem", pem("CERTIFICATE")),
        File("srv/app/public.pem", pem("PUBLIC KEY")),
        File(
            "var/lib/app/.ssh/id_ed25519.pub",
            b"ssh-ed25519 AAAAC3Nz user@host\n".to_vec(),
        ),
        File(
            "root/.docker/config.json",
            br#"{"auths":{"registry.example.invalid":{}}}"#.to_vec(),
        ),
        File(
            "root/.aws/config",
            b"[default]\nregion = eu-west-1\n".to_vec(),
        ),
        File(
            "var/lib/app/.npmrc",
            b"registry=https://registry.npmjs.org/\n".to_vec(),
        ),
        File(
            "var/lib/app/.netrc",
            b"machine example.invalid login anonymous\n".to_vec(),
        ),
        File("root/.git-credentials", Vec::new()),
        File(
            "srv/key-docs.md",
            b"A key starts with -----BEGIN PRIVATE KEY but this is prose.\n".to_vec(),
        ),
        Dir("srv/id_rsa"),
        Symlink("srv/link.key", "/srv/app/real.key"),
        File("srv/app/real.key", key()),
        HardLink("srv/app/copy.key", "srv/app/real.key"),
    ])?])?;
    assert_eq!(
        paths(&found),
        ["/srv/app/real.key"],
        "only the regular file itself"
    );
    Ok(())
}

#[test]
fn only_the_first_bytes_are_read_for_the_header() -> TestResult {
    let mut late = vec![b'#'; HEADER_BYTES];
    late.extend_from_slice(&key());
    let mut early = key();
    early.extend(vec![b'#'; 1 << 20]);
    let found = scan(&[layer(&[
        File("srv/late.pem", late),
        File("srv/early.pem", early),
    ])?])?;
    assert_eq!(
        paths(&found),
        ["/srv/early.pem"],
        "a header past the first 4 KiB is out of the scan's reach"
    );
    Ok(())
}

#[test]
fn a_later_layer_removes_replaces_or_adds() -> TestResult {
    let base = layer(&[
        File("srv/a/one.key", key()),
        File("srv/a/two.key", key()),
        File("srv/b/three.key", key()),
        File("srv/c/four.key", key()),
        File("srv/d/five.key", key()),
    ])?;
    let later = layer(&[
        // A whiteout removes one file; an opaque directory removes the lower layers' content of that directory only.
        File("srv/a/.wh.one.key", Vec::new()),
        File("srv/b/.wh..wh..opq", Vec::new()),
        File("srv/b/new.key", key()),
        // A whiteout of a directory removes everything under it.
        File("srv/.wh.c", Vec::new()),
        // A plain file over a key replaces it.
        File("srv/d/five.key", b"no longer a key\n".to_vec()),
    ])?;
    let found = scan(&[base, later])?;
    assert_eq!(paths(&found), ["/srv/a/two.key", "/srv/b/new.key"]);
    Ok(())
}

#[test]
fn at_most_sixteen_paths_are_listed_with_the_total() -> TestResult {
    let names: Vec<String> = (0..40)
        .map(|index| format!("srv/keys/{index:02}.key"))
        .collect();
    let entries: Vec<Entry<'_>> = names.iter().map(|name| File(name, key())).collect();
    let found = scan(&[layer(&entries)?])?;
    assert_eq!(found.items.len(), MAX_LISTED);
    assert_eq!(found.total, 40);
    assert_eq!(found.items[0].path, "/srv/keys/00.key");
    Ok(())
}

#[test]
fn past_either_bound_the_scan_stops_never_clean() -> TestResult {
    let entries: Vec<Entry<'_>> = (0..10).map(|_| Dir("srv")).collect();
    let many = layer(&entries)?;
    assert_eq!(
        scan_bounded(
            std::slice::from_ref(&many),
            Bounds {
                max_entries: 9,
                max_bytes: 1 << 30
            }
        ),
        Err(ScanError::Limit(Bound::Entries))
    );
    assert!(
        scan_bounded(
            std::slice::from_ref(&many),
            Bounds {
                max_entries: 10,
                max_bytes: 1 << 30
            }
        )
        .is_ok()
    );
    let big = layer(&[
        File("srv/blob", vec![0; 5000]),
        File("srv/more", vec![0; 5000]),
    ])?;
    assert_eq!(
        scan_bounded(
            &[big],
            Bounds {
                max_entries: 100,
                max_bytes: 9_999
            }
        ),
        Err(ScanError::Limit(Bound::Bytes))
    );
    Ok(())
}

#[test]
fn a_path_leaving_the_root_or_a_broken_archive_is_malformed() -> TestResult {
    assert_eq!(
        scan(&[b"not a tar archive, just text that is long enough to be read".repeat(20)]),
        Err(ScanError::Malformed)
    );
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(0);
    // `append_data` refuses `..`, so the name is written into the header directly.
    header.as_old_mut().name[..9].copy_from_slice(b"../escape");
    header.set_cksum();
    let mut builder = tar::Builder::new(Vec::new());
    builder.append(&header, &[][..])?;
    assert_eq!(scan(&[builder.into_inner()?]), Err(ScanError::Malformed));
    Ok(())
}

#[test]
fn no_layer_means_nothing_found() -> TestResult {
    let found = scan(&[])?;
    assert_eq!((found.items.len(), found.total), (0, 0));
    Ok(())
}

#[test]
fn listed_paths_are_sanitised_and_bounded() -> TestResult {
    let long = format!("/srv/{}é.key", "a".repeat(600));
    let raw = "/srv/new\nline\u{1b}.key".to_owned();
    let hits: BTreeMap<String, Rule> =
        [(raw.clone(), Rule::PrivateKey), (long, Rule::PrivateKey)].into();
    let listing = SecretFiles::listing(&hits, &BTreeSet::new());
    assert_eq!(listing.total, 2);
    let paths = listing.paths();
    assert!(
        paths.contains(&"/srv/new\u{fffd}line\u{fffd}.key"),
        "{paths:?}"
    );
    let cut = paths
        .iter()
        .find(|path| path.ends_with('…'))
        .ok_or("the long path is cut")?;
    assert!(cut.len() <= MAX_LISTED_PATH_BYTES);
    // Acknowledgement compares the raw paths, before the listing's sanitising.
    let marked = SecretFiles::listing(&hits, &[raw].into());
    assert_eq!(marked.unacknowledged, 1);
    assert!(
        marked
            .items
            .iter()
            .any(|item| item.acknowledged && item.path.contains("line"))
    );
    Ok(())
}

#[test]
fn each_hit_names_its_rule_and_acknowledged_ones_are_marked_after_the_rest() -> TestResult {
    let found = scan(&[layer(&[
        File("srv/app/server.key", pem("RSA PRIVATE KEY")),
        File(
            "root/.git-credentials",
            b"https://user:token@git.example.invalid\n".to_vec(),
        ),
        File("root/.ssh/id_rsa", pem("RSA PRIVATE KEY")),
        File("var/lib/app/.npmrc", b"//r/:_authToken=x\n".to_vec()),
    ])?])?;
    let rules: Vec<(&str, Rule)> = found
        .items
        .iter()
        .map(|item| (item.path.as_str(), item.rule))
        .collect();
    // An id_* file is an SSH key even when it also holds a PEM header.
    assert_eq!(
        rules,
        [
            ("/root/.git-credentials", Rule::GitCredentials),
            ("/root/.ssh/id_rsa", Rule::SshKey),
            ("/srv/app/server.key", Rule::PrivateKey),
            ("/var/lib/app/.npmrc", Rule::NpmrcToken),
        ]
    );
    let names: Vec<&str> = [
        Rule::PrivateKey,
        Rule::GitCredentials,
        Rule::Netrc,
        Rule::AwsCredentials,
        Rule::DockerAuth,
        Rule::NpmrcToken,
        Rule::PypircPassword,
        Rule::SshKey,
    ]
    .iter()
    .map(|rule| rule.as_str())
    .collect();
    assert_eq!(
        names,
        [
            "private_key",
            "git_credentials",
            "netrc",
            "aws_credentials",
            "docker_auth",
            "npmrc_token",
            "pypirc_password",
            "ssh_key"
        ]
    );
    let hits: BTreeMap<String, Rule> = [
        ("/a/acknowledged.key".to_owned(), Rule::PrivateKey),
        ("/z/unknown.key".to_owned(), Rule::PrivateKey),
    ]
    .into();
    let listing = SecretFiles::listing(&hits, &["/a/acknowledged.key".to_owned()].into());
    assert_eq!(
        listing.items,
        [
            SecretFile {
                path: "/z/unknown.key".into(),
                rule: Rule::PrivateKey,
                acknowledged: false
            },
            SecretFile {
                path: "/a/acknowledged.key".into(),
                rule: Rule::PrivateKey,
                acknowledged: true
            },
        ]
    );
    assert_eq!((listing.total, listing.unacknowledged), (2, 1));
    assert_eq!(
        (Bound::Entries.as_str(), Bound::Bytes.as_str()),
        ("entries", "bytes")
    );
    Ok(())
}
