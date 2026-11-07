use super::*;
use std::path::Path;

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn locations(directory: &Path) -> std::io::Result<Locations> {
    let paths = Locations {
        workspace: directory.join("workspace"),
        state: directory.join("state"),
    };
    fs::create_dir(&paths.workspace)?;
    fs::create_dir(&paths.state)?;
    Ok(paths)
}

#[test]
fn rt_i_015_checkout_install_preserves_files_modes_and_symlinks_without_following_them()
-> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir()?;
    let paths = locations(directory.path())?;
    let cancel = CancellationToken::new();
    assert_eq!(inspect(&paths)?, Initial::Empty);
    let stage = begin(&paths, &cancel)?;
    fs::create_dir(&stage)?;
    fs::create_dir(stage.join("nested"))?;
    fs::write(stage.join("nested/file"), b"nested fixture")?;
    fs::write(stage.join("script"), b"fixture")?;
    fs::set_permissions(stage.join("script"), fs::Permissions::from_mode(0o755))?;
    let outside = directory.path().join("outside");
    fs::write(&outside, b"unchanged")?;
    std::os::unix::fs::symlink(&outside, stage.join("link"))?;
    install(&paths, &cancel)?;
    assert_eq!(fs::read(paths.workspace.join("script"))?, b"fixture");
    assert_eq!(
        fs::read(paths.workspace.join("nested/file"))?,
        b"nested fixture"
    );
    assert_eq!(
        fs::metadata(paths.workspace.join("script"))?
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(fs::read_link(paths.workspace.join("link"))?, outside);
    assert_eq!(fs::read(directory.path().join("outside"))?, b"unchanged");
    assert!(!paths.staging().exists());
    assert_eq!(inspect(&paths)?, Initial::Attempted);
    Ok(())
}

#[test]
fn rt_i_015_checkout_never_replaces_preexisting_or_concurrently_added_content() -> TestResult {
    let directory = tempfile::tempdir()?;
    let paths = locations(directory.path())?;
    let existing = paths.workspace.join("existing");
    fs::write(&existing, b"keep")?;
    assert_eq!(inspect(&paths)?, Initial::Existing);
    assert!(begin(&paths, &CancellationToken::new()).is_err());
    assert_eq!(fs::read(&existing)?, b"keep");
    fs::remove_file(&existing)?;
    let stage = begin(&paths, &CancellationToken::new())?;
    fs::create_dir(&stage)?;
    fs::write(stage.join("existing"), b"checkout")?;
    fs::write(&existing, b"arrived during transfer")?;
    assert!(install(&paths, &CancellationToken::new()).is_err());
    assert_eq!(fs::read(&existing)?, b"arrived during transfer");
    Ok(())
}

#[test]
fn rt_i_015_interrupted_install_marker_and_cancel_fail_closed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let paths = locations(directory.path())?;
    let stage = begin(&paths, &CancellationToken::new())?;
    fs::create_dir(&stage)?;
    fs::write(stage.join("partial"), b"fixture")?;
    assert_eq!(inspect(&paths)?, Initial::Attempted);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(install(&paths, &cancel).is_err());
    assert!(!paths.workspace.join("partial").exists());
    assert!(begin(&paths, &CancellationToken::new()).is_err());
    Ok(())
}

#[test]
fn rt_i_015_checkout_refuses_a_special_file_without_installing_it() -> TestResult {
    // A socket's path must fit in 108 bytes, so this one stays out of a long TMPDIR (a mutants scratch copy, for one).
    let directory = tempfile::tempdir_in("/tmp")?;
    let paths = locations(directory.path())?;
    let cancel = CancellationToken::new();
    let stage = begin(&paths, &cancel)?;
    fs::create_dir(&stage)?;
    let _listener = std::os::unix::net::UnixListener::bind(stage.join("socket"))?;
    assert!(install(&paths, &cancel).is_err());
    assert!(!paths.workspace.join("socket").exists());
    assert_eq!(inspect(&paths)?, Initial::Attempted);
    Ok(())
}
