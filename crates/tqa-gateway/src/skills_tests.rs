//! GW-I-063 (startup): the install tier is compared with protected locations as resolved paths.
use super::{protected, resolve_install_tier};
use crate::GatewayError;
use std::path::{Path, PathBuf};

struct Host {
    _base: tempfile::TempDir,
    storage: PathBuf,
    secrets: PathBuf,
    skills: PathBuf,
}

fn host() -> Result<Host, Box<dyn std::error::Error>> {
    let base = tempfile::tempdir()?;
    let path = base.path().canonicalize()?;
    for name in ["storage", "secrets", "skills"] {
        std::fs::create_dir(path.join(name))?;
    }
    Ok(Host {
        storage: path.join("storage"),
        secrets: path.join("secrets"),
        skills: path.join("skills"),
        _base: base,
    })
}

fn guarded(host: &Host) -> Vec<PathBuf> {
    protected(
        &host.storage,
        &[&host.secrets.join("install.key"), Path::new("models.toml")],
    )
}

fn refused(result: &Result<PathBuf, GatewayError>) -> bool {
    matches!(result, Err(GatewayError::Configuration(_)))
}

#[tokio::test]
async fn gw_i_063_a_separate_directory_resolves_to_its_canonical_path()
-> Result<(), Box<dyn std::error::Error>> {
    let host = host()?;
    let link = host.storage.parent().ok_or("parent")?.join("skills-link");
    std::os::unix::fs::symlink(&host.skills, &link)?;
    assert_eq!(
        resolve_install_tier(&link, &guarded(&host)).await?,
        host.skills
    );
    Ok(())
}

#[tokio::test]
async fn gw_i_063_symlinks_and_overlaps_with_protected_locations_are_refused()
-> Result<(), Box<dyn std::error::Error>> {
    let host = host()?;
    let base = host.storage.parent().ok_or("parent")?.to_path_buf();
    std::os::unix::fs::symlink(&host.storage, base.join("into-storage"))?;
    std::fs::create_dir(host.secrets.join("inside"))?;
    std::os::unix::fs::symlink(&host.secrets, host.skills.join("secrets"))?;
    for root in [
        base.join("into-storage"),
        host.storage.clone(),
        host.secrets.join("inside"),
        base.clone(),
        std::env::current_dir()?,
    ] {
        assert!(
            refused(&resolve_install_tier(&root, &guarded(&host)).await),
            "{} accepted",
            root.display()
        );
    }
    // A link inside the tier to a protected directory is the operator's content; the tier itself stays separate.
    assert!(
        resolve_install_tier(&host.skills, &guarded(&host))
            .await
            .is_ok()
    );
    Ok(())
}

#[tokio::test]
async fn gw_i_063_a_missing_root_or_a_file_is_refused() -> Result<(), Box<dyn std::error::Error>> {
    let host = host()?;
    std::fs::write(host.skills.join("file"), b"x")?;
    for root in [host.skills.join("absent"), host.skills.join("file")] {
        assert!(refused(&resolve_install_tier(&root, &guarded(&host)).await));
    }
    Ok(())
}
