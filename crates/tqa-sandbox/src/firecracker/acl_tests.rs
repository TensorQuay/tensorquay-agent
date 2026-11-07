use std::{
    error::Error,
    fs::{File, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    process::Command,
};

use super::{Access, encode, grant, probe, set_error};

fn private_file(path: &Path) -> Result<File, Box<dyn Error>> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(b"secret")?;
    Ok(file)
}

fn getfacl(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let output = Command::new("getfacl")
        .args(["-n", "--omit-header", "-p"])
        .arg(path)
        .output()?;
    assert!(output.status.success(), "getfacl failed");
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

#[test]
fn sbx_f_011_acl_encoding_round_trips_through_getfacl() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    for (access, bits) in [(Access::Read, "r--"), (Access::ReadWrite, "rw-")] {
        let path = directory.path().join(format!("{bits}.img"));
        let file = private_file(&path)?;
        grant(&file, 4242, access)?;
        grant(&file, 4242, access)?; // idempotent
        assert_eq!(
            getfacl(&path)?,
            [
                "user::rw-",
                &format!("user:4242:{bits}"),
                "group::---",
                &format!("mask::{bits}"),
                "other::---"
            ]
        );
        // The group bits of the mode now show the mask; the owning group's own entry stays empty.
        assert_eq!(
            std::fs::metadata(&path)?.permissions().mode() & 0o777,
            if bits == "r--" { 0o640 } else { 0o660 }
        );
    }
    Ok(())
}

#[test]
fn sbx_f_011_acl_encoding_is_sorted_and_versioned() {
    let bytes = encode(7, Access::Read);
    assert_eq!(bytes.len(), 4 + 5 * 8);
    assert_eq!(bytes[..4], 2u32.to_le_bytes());
    let tags: Vec<u16> = bytes[4..]
        .chunks(8)
        .map(|entry| u16::from_le_bytes([entry[0], entry[1]]))
        .collect();
    assert_eq!(tags, [0x01, 0x02, 0x04, 0x10, 0x20]);
}

#[test]
fn sbx_f_001_acl_probe_leaves_nothing_behind_and_refuses_root() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    probe(directory.path(), 4242)?;
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);
    let file = private_file(&directory.path().join("x"))?;
    assert!(
        grant(&file, 0, Access::Read).is_err(),
        "root is never granted"
    );
    Ok(())
}

#[test]
fn sbx_f_001_missing_acl_support_fails_closed_with_its_reason() {
    assert_eq!(
        set_error(rustix::io::Errno::OPNOTSUPP).to_string(),
        "sandbox preflight failed: storage.root does not support POSIX ACLs"
    );
    assert_eq!(set_error(rustix::io::Errno::PERM).code(), "sandbox.backend");
}

/// The uid this process must grant so that container uid `inner` of this user's rootless engine can read a file. On
/// the host that is the subordinate range start + inner - 1; inside the engine's own user namespace (as under
/// tools/rootless-test.sh) container uids appear unchanged.
fn mapped(inner: u32) -> Result<u32, Box<dyn Error>> {
    let identity = std::fs::read_to_string("/proc/self/uid_map")?
        .split_whitespace()
        .collect::<Vec<_>>()
        == ["0", "0", "4294967295"];
    if !identity {
        return Ok(inner);
    }
    let user = String::from_utf8(Command::new("id").arg("-un").output()?.stdout)?
        .trim()
        .to_owned();
    let line = std::fs::read_to_string("/etc/subuid")?
        .lines()
        .find(|line| line.starts_with(&format!("{user}:")))
        .map(str::to_owned)
        .ok_or("no subordinate uid range")?;
    let start: u32 = line
        .split(':')
        .nth(1)
        .ok_or("malformed /etc/subuid")?
        .parse()?;
    Ok(start + inner - 1)
}

#[test]
#[ignore = "docker"]
fn sbx_f_011_only_the_granted_uid_can_read() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("config.img");
    let file = private_file(&path)?;
    grant(&file, mapped(10_000)?, Access::Read)?;
    let read_as = |uid: &str| -> Result<(bool, String), Box<dyn Error>> {
        let output = Command::new("docker")
            .args(["run", "--rm", "--network", "none", "--user", uid, "-v"])
            .arg(format!("{}:/f:ro", path.display()))
            .args(["alpine:latest", "cat", "/f"])
            .output()?;
        Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    };
    assert!(read_as("10000:10000")?.0, "the VMM uid reads the file");
    for other in ["10001:10001", "1000:1000", "10001:10000"] {
        let (read, stderr) = read_as(other)?;
        assert!(
            !read && stderr.contains("Permission denied"),
            "{other} must get EACCES: {stderr}"
        );
    }
    Ok(())
}
