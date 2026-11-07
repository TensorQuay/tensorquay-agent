use std::{error::Error, fs, path::Path};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{RootfsStatus, Verified, hex, kernel, locate, rootfs, status, verify_bound};
use crate::{ImageRef, SandboxError};

const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const IMAGE: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn kernel_manifest(vmlinux: &[u8]) -> Value {
    json!({
        "schema": 1, "release": "6.18.54", "licence": "GPL-2.0-only", "source_sha256": ZERO, "base_config": "b",
        "base_config_sha256": ZERO, "fragment_sha256": ZERO, "config_sha256": ZERO, "vmlinux_sha256": digest(vmlinux)
    })
}

fn rootfs_manifest(image: &str, content: &[u8], revision: u16) -> Value {
    built_from(image, content, revision, &[])
}

/// A manifest for an image the builder saw under these registry digests (`repo@sha256:…`).
fn built_from(image: &str, content: &[u8], revision: u16, repo_digests: &[&str]) -> Value {
    json!({
        "schema": 2,
        "source_image": { "id": image, "repo_digests": repo_digests, "protocol_revision": revision },
        "rootfs": { "file": "rootfs.ext4", "sha256": digest(content), "bytes": content.len(), "tree_sha256": ZERO },
        "init": { "path": "/usr/sbin/tqa-vm-init", "sha256": ZERO },
        "runtime": { "path": "/usr/local/bin/tqa-runtime", "sha256": ZERO },
        "builder": { "image_id": format!("sha256:{ZERO}"), "e2fsprogs": "1.47.2" },
        "image": { "env_keys": ["HOME", "PATH"], "volumes": [] }
    })
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(path.parent().ok_or("no parent")?)?;
    Ok(fs::write(path, bytes)?)
}

fn preflight_reason<T>(result: &Result<T, SandboxError>) -> Option<&'static str> {
    match result {
        Err(SandboxError::Preflight(reason)) => Some(*reason),
        _ => None,
    }
}

#[test]
fn sbx_f_001_kernel_is_verified_against_its_manifest() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let verified = Verified::default();
    let missing = kernel(directory.path(), &verified);
    assert_eq!(
        preflight_reason(&missing),
        Some("the guest kernel manifest is missing or malformed")
    );
    write(&directory.path().join("vmlinux"), b"kernel")?;
    write(
        &directory.path().join("manifest.json"),
        &serde_json::to_vec(&kernel_manifest(b"kernel"))?,
    )?;
    assert_eq!(kernel(directory.path(), &verified)?.release, "6.18.54");
    // A changed file is re-verified even though an earlier check succeeded.
    write(&directory.path().join("vmlinux"), b"kernel, patched")?;
    let changed = kernel(directory.path(), &verified);
    assert_eq!(
        preflight_reason(&changed),
        Some("the guest kernel does not match its manifest")
    );
    let mut unknown = kernel_manifest(b"kernel");
    unknown["extra"] = json!(1);
    write(
        &directory.path().join("manifest.json"),
        &serde_json::to_vec(&unknown)?,
    )?;
    let malformed = kernel(directory.path(), &verified);
    assert_eq!(
        preflight_reason(&malformed),
        Some("the guest kernel manifest is missing or malformed")
    );
    Ok(())
}

#[test]
fn sbx_f_001_rootfs_is_verified_for_its_image() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let verified = Verified::default();
    let image = ImageRef::local_id(IMAGE)?;
    let base = directory.path().join(IMAGE.trim_start_matches("sha256:"));
    let absent = rootfs(directory.path(), &image, None, &verified);
    assert_eq!(
        preflight_reason(&absent),
        Some("no rootfs was built for this image")
    );
    write(&base.join("rootfs.ext4"), b"ext4")?;
    let revision = tqa_types::PROTOCOL_REVISION;
    write(
        &base.join("manifest.json"),
        &serde_json::to_vec(&rootfs_manifest(IMAGE, b"ext4", revision))?,
    )?;
    assert_eq!(
        rootfs(directory.path(), &image, None, &verified)?.protocol_revision,
        revision
    );

    let other = format!("sha256:{}", "2".repeat(64));
    write(
        &base.join("manifest.json"),
        &serde_json::to_vec(&rootfs_manifest(&other, b"ext4", revision))?,
    )?;
    let foreign = rootfs(directory.path(), &image, None, &verified);
    assert_eq!(
        preflight_reason(&foreign),
        Some("the rootfs manifest names a different image")
    );

    write(
        &base.join("manifest.json"),
        &serde_json::to_vec(&rootfs_manifest(IMAGE, b"ext5", revision))?,
    )?;
    let mismatch = rootfs(directory.path(), &image, None, &verified);
    assert_eq!(
        preflight_reason(&mismatch),
        Some("the rootfs does not match its manifest")
    );

    write(
        &base.join("manifest.json"),
        &serde_json::to_vec(&rootfs_manifest(IMAGE, b"ext4", 1))?,
    )?;
    assert!(matches!(
        rootfs(directory.path(), &image, None, &verified),
        Err(SandboxError::RuntimeTooOld { has: 1, .. })
    ));
    Ok(())
}

#[test]
fn sbx_f_001_the_rootfs_check_is_bounded_by_the_rootfs_size() -> Result<(), Box<dyn Error>> {
    use std::time::Duration;
    let directory = tempfile::tempdir()?;
    let image = ImageRef::local_id(format!("sha256:{}", "c".repeat(64)))?;
    let base = Duration::from_secs(30);
    assert_eq!(
        verify_bound(directory.path(), &image, base),
        base,
        "no rootfs: the base bound"
    );
    let rootfs = directory.path().join("c".repeat(64));
    fs::create_dir(&rootfs)?;
    // Sparse: 100 MiB at the slowest rate (20 MiB/s) adds 5 s; one more byte adds a second (rounded up).
    fs::File::create(rootfs.join("rootfs.ext4"))?.set_len(100 << 20)?;
    assert_eq!(
        verify_bound(directory.path(), &image, base),
        base + Duration::from_secs(5)
    );
    fs::File::create(rootfs.join("rootfs.ext4"))?.set_len((100 << 20) + 1)?;
    assert_eq!(
        verify_bound(directory.path(), &image, base),
        base + Duration::from_secs(6)
    );
    Ok(())
}

#[test]
fn sbx_c_014_a_rootfs_from_an_older_builder_must_be_rebuilt() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let image = ImageRef::local_id(format!("sha256:{}", "d".repeat(64)))?;
    let content = b"ext4";
    let base = directory.path().join("d".repeat(64));
    write(&base.join("rootfs.ext4"), content)?;
    let mut old = rootfs_manifest(image.digest(), content, tqa_types::PROTOCOL_REVISION);
    old["schema"] = json!(1);
    if let Some(fields) = old.as_object_mut() {
        fields.remove("image");
    }
    write(&base.join("manifest.json"), &serde_json::to_vec(&old)?)?;
    let refused = rootfs(directory.path(), &image, None, &Verified::default());
    assert_eq!(
        preflight_reason(&refused),
        Some("the rootfs was built by an older builder; rebuild it (images/rootfs)")
    );
    Ok(())
}

/// The registry digest the fixtures pin: a manifest digest, which is not the image ID.
const PINNED: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn build(root: &Path, image: &str, repo_digests: &[&str]) -> Result<(), Box<dyn Error>> {
    let base = root.join(image.trim_start_matches("sha256:"));
    write(&base.join("rootfs.ext4"), b"ext4")?;
    write(
        &base.join("manifest.json"),
        &serde_json::to_vec(&built_from(
            image,
            b"ext4",
            tqa_types::PROTOCOL_REVISION,
            repo_digests,
        ))?,
    )
}

#[test]
fn sbx_f_022_a_registry_pin_locates_the_rootfs_built_for_its_image_id() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    let verified = Verified::default();
    let listed = format!("registry.example/team/template@{PINNED}");
    build(directory.path(), IMAGE, &[&listed])?;
    // Another image's build beside it, and entries that are not builds, are passed over.
    build(
        directory.path(),
        &format!("sha256:{}", "3".repeat(64)),
        &[
            "registry.example/team/other@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ],
    )?;
    write(&directory.path().join("README"), b"not a build")?;
    fs::create_dir(directory.path().join("not-a-digest"))?;
    let pin = ImageRef::new("registry.example/team/template", PINNED)?;
    let located = locate(directory.path(), &pin)?;
    assert!(located.is_local());
    assert_eq!(located.digest(), IMAGE);
    assert_eq!(
        rootfs(directory.path(), &located, None, &verified)?.path,
        directory
            .path()
            .join(IMAGE.trim_start_matches("sha256:"))
            .join("rootfs.ext4")
    );
    // The digest identifies the content, whatever name the registry was reached by.
    let mirrored = ImageRef::new("mirror.example/template", PINNED)?;
    assert_eq!(locate(directory.path(), &mirrored)?.digest(), IMAGE);
    // A local pin is already the key.
    let local = ImageRef::local_id(IMAGE)?;
    assert_eq!(locate(directory.path(), &local)?, local);
    Ok(())
}

#[test]
fn sbx_f_022_a_registry_pin_without_exactly_one_build_is_refused() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let pin = ImageRef::new("registry.example/team/template", PINNED)?;
    assert_eq!(
        preflight_reason(&locate(directory.path(), &pin)),
        Some("no rootfs was built for this image")
    );
    assert_eq!(
        preflight_reason(&locate(&directory.path().join("missing"), &pin)),
        Some("no rootfs was built for this image")
    );
    let listed = format!("registry.example/team/template@{PINNED}");
    build(directory.path(), IMAGE, &[&listed])?;
    build(
        directory.path(),
        &format!("sha256:{}", "4".repeat(64)),
        &[&listed],
    )?;
    assert_eq!(
        preflight_reason(&locate(directory.path(), &pin)),
        Some("several rootfs builds claim this image")
    );
    // A manifest copied into another image's directory still names its own image.
    let moved = tempfile::tempdir()?;
    let base = moved.path().join("5".repeat(64));
    write(&base.join("rootfs.ext4"), b"ext4")?;
    write(
        &base.join("manifest.json"),
        &serde_json::to_vec(&built_from(
            IMAGE,
            b"ext4",
            tqa_types::PROTOCOL_REVISION,
            &[&listed],
        ))?,
    )?;
    let located = locate(moved.path(), &pin)?;
    assert_eq!(
        preflight_reason(&rootfs(moved.path(), &located, None, &Verified::default())),
        Some("the rootfs manifest names a different image")
    );
    Ok(())
}

#[test]
fn sbx_f_022_the_search_for_a_registry_pin_is_bounded() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    for index in 0..=super::MAX_ROOTFS_BUILDS {
        fs::create_dir(directory.path().join(format!("{index:064x}")))?;
    }
    let pin = ImageRef::new("registry.example/team/template", PINNED)?;
    assert_eq!(
        preflight_reason(&locate(directory.path(), &pin)),
        Some("too many rootfs builds to search for this image")
    );
    Ok(())
}

#[test]
fn sbx_f_001_malformed_oversized_and_resized_artefacts_are_refused() -> Result<(), Box<dyn Error>> {
    let verified = Verified::default();
    // A kernel manifest that parses but breaks its rules (another licence).
    let kernels = tempfile::tempdir()?;
    write(&kernels.path().join("vmlinux"), b"kernel")?;
    let mut document = kernel_manifest(b"kernel");
    document["licence"] = json!("MIT");
    write(
        &kernels.path().join("manifest.json"),
        &serde_json::to_vec(&document)?,
    )?;
    assert_eq!(
        preflight_reason(&kernel(kernels.path(), &verified)),
        Some("the guest kernel manifest is missing or malformed")
    );
    // A manifest past the size bound is refused before it is parsed.
    write(
        &kernels.path().join("manifest.json"),
        &vec![b' '; usize::try_from(super::MAX_MANIFEST_BYTES + 1)?],
    )?;
    assert_eq!(
        preflight_reason(&kernel(kernels.path(), &verified)),
        Some("the guest kernel manifest is missing or malformed")
    );
    let directory = tempfile::tempdir()?;
    let image = ImageRef::local_id(IMAGE)?;
    let base = directory.path().join(IMAGE.trim_start_matches("sha256:"));
    write(&base.join("rootfs.ext4"), b"ext4")?;
    let revision = tqa_types::PROTOCOL_REVISION;
    // A rootfs manifest that parses but breaks its rules (another file name).
    let mut document = rootfs_manifest(IMAGE, b"ext4", revision);
    document["rootfs"]["file"] = json!("other.ext4");
    write(&base.join("manifest.json"), &serde_json::to_vec(&document)?)?;
    assert_eq!(
        preflight_reason(&rootfs(directory.path(), &image, None, &verified)),
        Some("the rootfs manifest is malformed")
    );
    // A rootfs whose size differs from its manifest is refused before it is hashed.
    write(
        &base.join("manifest.json"),
        &serde_json::to_vec(&rootfs_manifest(IMAGE, b"ext4", revision))?,
    )?;
    write(&base.join("rootfs.ext4"), b"ext4 grown")?;
    assert_eq!(
        preflight_reason(&rootfs(directory.path(), &image, None, &verified)),
        Some("the rootfs does not match its manifest")
    );
    Ok(())
}

#[test]
fn sbx_f_001_an_artefact_that_cannot_be_read_is_refused() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    let reason = Some("the guest kernel does not match its manifest");
    let directory = tempfile::tempdir()?;
    write(
        &directory.path().join("manifest.json"),
        &serde_json::to_vec(&kernel_manifest(b"kernel"))?,
    )?;
    // Missing.
    let missing = kernel(directory.path(), &Verified::default());
    // A directory where the file should be: it opens, but reading fails.
    fs::create_dir(directory.path().join("vmlinux"))?;
    let unreadable_kind = kernel(directory.path(), &Verified::default());
    fs::remove_dir(directory.path().join("vmlinux"))?;
    // A socket where the file should be: opening it fails for every user, root included.
    let socket = std::os::unix::net::UnixListener::bind(directory.path().join("vmlinux"))?;
    let unopenable = kernel(directory.path(), &Verified::default());
    drop(socket);
    fs::remove_file(directory.path().join("vmlinux"))?;
    for result in [&missing, &unreadable_kind, &unopenable] {
        assert_eq!(preflight_reason(result), reason, "{result:?}");
    }
    // A file the process may not open, by its mode. Root, or root in a user namespace as under tools/rootless-test.sh
    // (CAP_DAC_OVERRIDE), opens a mode-000 file anyway, so there this case proves nothing and is left out; the socket
    // above is the unreadable file that holds for every user.
    write(&directory.path().join("vmlinux"), b"kernel")?;
    fs::set_permissions(
        directory.path().join("vmlinux"),
        fs::Permissions::from_mode(0o000),
    )?;
    if fs::File::open(directory.path().join("vmlinux")).is_err() {
        let closed = kernel(directory.path(), &Verified::default());
        assert_eq!(preflight_reason(&closed), reason, "{closed:?}");
    }
    fs::set_permissions(
        directory.path().join("vmlinux"),
        fs::Permissions::from_mode(0o600),
    )?;
    Ok(())
}

/// A kernel directory whose manifest is `document` padded with trailing spaces (valid JSON) to exactly `bytes`.
fn padded_kernel(bytes: usize) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    write(&directory.path().join("vmlinux"), b"kernel")?;
    let mut text = serde_json::to_vec(&kernel_manifest(b"kernel"))?;
    text.resize(bytes, b' ');
    write(&directory.path().join("manifest.json"), &text)?;
    Ok(directory)
}

#[test]
fn sbx_f_001_a_manifest_may_use_exactly_the_size_bound() -> Result<(), Box<dyn Error>> {
    let bound = usize::try_from(super::MAX_MANIFEST_BYTES)?;
    assert_eq!(bound, 64 * 1024);
    let exact = padded_kernel(bound)?;
    assert!(kernel(exact.path(), &Verified::default()).is_ok());
    // One byte more is refused even though the first 64 KiB alone would parse.
    let over = padded_kernel(bound + 1)?;
    assert_eq!(
        preflight_reason(&kernel(over.path(), &Verified::default())),
        Some("the guest kernel manifest is missing or malformed")
    );
    Ok(())
}

#[test]
fn sbx_f_022_exactly_the_search_bound_of_builds_is_still_searched() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    build(
        directory.path(),
        IMAGE,
        &[&format!("registry.example/team/template@{PINNED}")],
    )?;
    for index in 1..super::MAX_ROOTFS_BUILDS {
        fs::create_dir(directory.path().join(format!("{index:064x}")))?;
    }
    let pin = ImageRef::new("registry.example/team/template", PINNED)?;
    assert_eq!(locate(directory.path(), &pin)?.digest(), IMAGE);
    Ok(())
}

#[test]
fn sbx_c_014_a_malformed_image_policy_makes_the_manifest_malformed() -> Result<(), Box<dyn Error>> {
    let many: Vec<String> = (0..=256).map(|index| format!("K{index}")).collect();
    let most = &many[..256];
    let long = "K".repeat(1025);
    for (env_keys, volumes, accepted) in [
        (json!(most), json!(most), true),
        (json!(["K".repeat(1024)]), json!([]), true),
        (json!(many), json!([]), false),
        (json!([]), json!(many), false),
        (json!(["HOME", ""]), json!([]), false),
        (json!(["HOME"]), json!([""]), false),
        (json!([long]), json!([]), false),
    ] {
        let directory = tempfile::tempdir()?;
        let base = directory.path().join(IMAGE.trim_start_matches("sha256:"));
        write(&base.join("rootfs.ext4"), b"ext4")?;
        let mut document = rootfs_manifest(IMAGE, b"ext4", tqa_types::PROTOCOL_REVISION);
        document["image"] = json!({ "env_keys": env_keys, "volumes": volumes });
        write(&base.join("manifest.json"), &serde_json::to_vec(&document)?)?;
        let result = rootfs(
            directory.path(),
            &ImageRef::local_id(IMAGE)?,
            None,
            &Verified::default(),
        );
        if accepted {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert_eq!(
                preflight_reason(&result),
                Some("the rootfs manifest is malformed")
            );
        }
    }
    Ok(())
}

#[test]
fn sbx_f_001_a_manifest_digest_must_be_64_lowercase_hex() -> Result<(), Box<dyn Error>> {
    for digest in ["g".repeat(64), "abc".to_owned(), "A".repeat(64)] {
        let directory = tempfile::tempdir()?;
        let base = directory.path().join(IMAGE.trim_start_matches("sha256:"));
        write(&base.join("rootfs.ext4"), b"ext4")?;
        let mut document = rootfs_manifest(IMAGE, b"ext4", tqa_types::PROTOCOL_REVISION);
        document["rootfs"]["tree_sha256"] = json!(digest);
        write(&base.join("manifest.json"), &serde_json::to_vec(&document)?)?;
        assert_eq!(
            preflight_reason(&rootfs(
                directory.path(),
                &ImageRef::local_id(IMAGE)?,
                None,
                &Verified::default()
            )),
            Some("the rootfs manifest is malformed"),
            "{digest}"
        );
    }
    Ok(())
}

#[test]
fn sbx_f_023_each_rootfs_has_one_status() -> Result<(), Box<dyn Error>> {
    let builder = format!("sha256:{ZERO}");
    let verified = Verified::default();
    let local = ImageRef::local_id(IMAGE)?;
    let pin = ImageRef::new("registry.example/team/template", PINNED)?;
    let listed = format!("registry.example/team/template@{PINNED}");
    let directory = tempfile::tempdir()?;
    let at = |image: &ImageRef, builder: Option<&str>| {
        status(directory.path(), image, builder, &verified)
    };
    assert_eq!(at(&local, Some(&builder)), RootfsStatus::Missing);
    assert_eq!(at(&pin, Some(&builder)), RootfsStatus::Missing);
    build(directory.path(), IMAGE, &[&listed])?;
    assert_eq!(at(&local, Some(&builder)), RootfsStatus::Ready);
    assert_eq!(at(&pin, Some(&builder)), RootfsStatus::Ready);
    // Built by another VM host image: its guest init may not fit this one. Unknown builder: not compared.
    let other = format!("sha256:{}", "9".repeat(64));
    assert_eq!(at(&local, Some(&other)), RootfsStatus::Outdated);
    assert_eq!(at(&local, None), RootfsStatus::Ready);
    let base = directory.path().join(IMAGE.trim_start_matches("sha256:"));
    let manifest = base.join("manifest.json");
    let revision = tqa_types::PROTOCOL_REVISION;
    for (document, expected) in [
        // Schema 1 and a runtime below the protocol floor.
        (
            {
                let mut d = built_from(IMAGE, b"ext4", revision, &[&listed]);
                d["schema"] = json!(1);
                d
            },
            RootfsStatus::Outdated,
        ),
        (
            built_from(IMAGE, b"ext4", 1, &[&listed]),
            RootfsStatus::Outdated,
        ),
        // Another image's manifest in this directory.
        (
            built_from(
                &format!("sha256:{}", "6".repeat(64)),
                b"ext4",
                revision,
                &[],
            ),
            RootfsStatus::Foreign,
        ),
        // A hash and a size that differ from the file, and a manifest that breaks its rules.
        (
            built_from(IMAGE, b"ext5", revision, &[&listed]),
            RootfsStatus::Mismatch,
        ),
        (
            built_from(IMAGE, b"ext4 longer", revision, &[&listed]),
            RootfsStatus::Mismatch,
        ),
        (
            {
                let mut d = built_from(IMAGE, b"ext4", revision, &[&listed]);
                d["rootfs"]["file"] = json!("x");
                d
            },
            RootfsStatus::Mismatch,
        ),
    ] {
        write(&manifest, &serde_json::to_vec(&document)?)?;
        assert_eq!(at(&local, Some(&builder)), expected, "{document}");
    }
    // Two builds claiming one registry digest.
    write(
        &manifest,
        &serde_json::to_vec(&built_from(IMAGE, b"ext4", revision, &[&listed]))?,
    )?;
    build(
        directory.path(),
        &format!("sha256:{}", "7".repeat(64)),
        &[&listed],
    )?;
    assert_eq!(at(&pin, Some(&builder)), RootfsStatus::Ambiguous);
    Ok(())
}

#[test]
fn sbx_f_023_create_and_status_agree_on_a_rootfs_from_another_builder() -> Result<(), Box<dyn Error>>
{
    let directory = tempfile::tempdir()?;
    build(directory.path(), IMAGE, &[])?;
    let image = ImageRef::local_id(IMAGE)?;
    let verified = Verified::default();
    let recorded = format!("sha256:{ZERO}");
    let other = format!("sha256:{}", "9".repeat(64));
    // The guest init comes from the builder: another VM host image's rootfs is refused by name, with the remedy.
    assert_eq!(
        preflight_reason(&rootfs(directory.path(), &image, Some(&other), &verified)),
        Some(
            "the rootfs was built by another VM host image; rebuild it with images/rootfs/build_rootfs.py"
        )
    );
    assert_eq!(
        status(directory.path(), &image, Some(&other), &verified),
        RootfsStatus::Outdated
    );
    // The recorded builder, or a VM host image whose ID is unknown (pinned by registry digest), is accepted.
    assert!(rootfs(directory.path(), &image, Some(&recorded), &verified).is_ok());
    assert!(rootfs(directory.path(), &image, None, &verified).is_ok());
    assert_eq!(
        status(directory.path(), &image, Some(&recorded), &verified),
        RootfsStatus::Ready
    );
    Ok(())
}
